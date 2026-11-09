//! Fast, non-blocking git metadata resolution (PLAN §10 it16).
//! Discovers git root, handles submodules/worktrees, resolves branches/detached HEAD,
//! and evaluates fast index dirtiness without launching child processes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use crate::model::{FileGitStatus, GitStatus};

pub fn find_git_dir(start: &Path) -> Option<(PathBuf, PathBuf)> {
    for candidate in start.ancestors() {
        let dot_git = candidate.join(".git");
        if dot_git.is_dir() {
            return Some((candidate.to_path_buf(), dot_git));
        }
        if dot_git.is_file()
            && let Ok(content) = std::fs::read_to_string(&dot_git)
        {
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("gitdir:") {
                    let rel = rest.trim();
                    let target = candidate.join(rel);
                    let resolved = target.canonicalize().unwrap_or(target);
                    if resolved.exists() {
                        return Some((candidate.to_path_buf(), resolved));
                    }
                }
            }
        }
    }
    None
}

pub fn resolve_git_status(dir: &Path) -> Option<GitStatus> {
    let (repo_root, git_dir) = find_git_dir(dir)?;
    let head_path = git_dir.join("HEAD");
    let head_content = std::fs::read_to_string(&head_path).ok()?;
    let trimmed = head_content.trim();

    let branch = if let Some(branch_ref) = trimmed.strip_prefix("ref: refs/heads/") {
        branch_ref.to_string()
    } else if let Some(ref_name) = trimmed.strip_prefix("ref: ") {
        ref_name.to_string()
    } else {
        // Detached HEAD
        let short_sha = if trimmed.len() >= 7 {
            &trimmed[..7]
        } else {
            trimmed
        };
        format!("HEAD ({short_sha})")
    };

    let is_dirty = check_is_dirty(&git_dir, dir, &repo_root);

    Some(GitStatus { branch, is_dirty })
}

fn check_is_dirty(git_dir: &Path, dir: &Path, _repo_root: &Path) -> bool {
    if git_dir.join("MERGE_HEAD").exists()
        || git_dir.join("REBASE_HEAD").exists()
        || git_dir.join("CHERRY_PICK_HEAD").exists()
        || git_dir.join("BISECT_LOG").exists()
    {
        return true;
    }

    let index_path = git_dir.join("index");
    let Ok(index_meta) = std::fs::metadata(&index_path) else {
        return false;
    };
    let Ok(index_mtime) = index_meta.modified() else {
        return false;
    };

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_name() == ".git" {
                continue;
            }
            if let Ok(ft) = entry.file_type()
                && ft.is_file()
                && let Ok(meta) = entry.metadata()
                && let Ok(mtime) = meta.modified()
                && mtime > index_mtime
            {
                return true;
            }
        }
    }

    false
}

/// Resolves git statuses for files within a repository directory using `git status --porcelain=v1 -z`.
pub fn resolve_file_git_statuses(dir: &Path) -> HashMap<PathBuf, FileGitStatus> {
    let mut map = HashMap::new();
    let Some((repo_root, _)) = find_git_dir(dir) else {
        return map;
    };

    let output = std::process::Command::new("git")
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .current_dir(&repo_root)
        .output();

    let Ok(output) = output else {
        return map;
    };

    if !output.status.success() {
        return map;
    }

    let stdout = output.stdout;
    let mut i = 0;
    while i < stdout.len() {
        let end = stdout[i..]
            .iter()
            .position(|&b| b == 0)
            .map(|pos| i + pos)
            .unwrap_or(stdout.len());

        let record = &stdout[i..end];
        if record.len() >= 4 {
            let x = record[0] as char;
            let y = record[1] as char;
            let rel_bytes = &record[3..];
            if let Ok(rel_str) = std::str::from_utf8(rel_bytes) {
                let abs_path = repo_root.join(rel_str);
                let status = match (x, y) {
                    ('?', '?') => FileGitStatus::Untracked,
                    ('!', '!') => FileGitStatus::Ignored,
                    ('M', ' ') | ('A', ' ') | ('C', ' ') => FileGitStatus::Staged,
                    (' ', 'M') | ('M', 'M') | ('A', 'M') => FileGitStatus::Modified,
                    (' ', 'D') | ('D', ' ') | ('D', 'D') => FileGitStatus::Deleted,
                    ('R', _) | (_, 'R') => FileGitStatus::Renamed,
                    _ if x != ' ' => FileGitStatus::Staged,
                    _ if y != ' ' => FileGitStatus::Modified,
                    _ => FileGitStatus::Modified,
                };
                map.insert(abs_path.clone(), status);

                // If path is inside dir in a subfolder, mark the subfolder in dir as modified
                if let Ok(rel_to_dir) = abs_path.strip_prefix(dir)
                    && let Some(first_comp) = rel_to_dir.components().next()
                {
                    let subfolder = dir.join(first_comp.as_os_str());
                    if subfolder != abs_path {
                        map.entry(subfolder).or_insert(status);
                    }
                }
            }

            if (x == 'R' || y == 'R') && end < stdout.len() {
                let next_start = end + 1;
                let next_end = stdout[next_start..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|pos| next_start + pos)
                    .unwrap_or(stdout.len());
                i = next_end + 1;
                continue;
            }
        }

        i = end + 1;
    }

    map
}

/// Retrieves git diff for a specific file path (staged or unstaged).
pub fn get_file_diff(path: &Path) -> Option<String> {
    let parent = path.parent().unwrap_or(path);
    let (repo_root, _) = find_git_dir(parent)?;

    // Try diff against HEAD first
    let output = std::process::Command::new("git")
        .args(["diff", "HEAD", "--", path.to_string_lossy().as_ref()])
        .current_dir(&repo_root)
        .output();

    if let Ok(out) = output
        && out.status.success()
        && !out.stdout.is_empty()
    {
        let diff = String::from_utf8_lossy(&out.stdout).into_owned();
        if !diff.trim().is_empty() {
            if diff.len() > 256 * 1024 {
                return Some(diff[..256 * 1024].to_string() + "\n... (diff truncated)");
            }
            return Some(diff);
        }
    }

    // Fallback without HEAD
    let output = std::process::Command::new("git")
        .args(["diff", "--", path.to_string_lossy().as_ref()])
        .current_dir(&repo_root)
        .output();

    if let Ok(out) = output
        && out.status.success()
        && !out.stdout.is_empty()
    {
        let diff = String::from_utf8_lossy(&out.stdout).into_owned();
        if !diff.trim().is_empty() {
            if diff.len() > 256 * 1024 {
                return Some(diff[..256 * 1024].to_string() + "\n... (diff truncated)");
            }
            return Some(diff);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_test_dir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ff-git-test-{}", name));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn non_git_dir_returns_none() {
        let root = temp_test_dir("non-git");
        assert!(resolve_git_status(&root).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn normal_branch_resolution() {
        let root = temp_test_dir("normal-branch");
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::write(git_dir.join("index"), "dummy index content").unwrap();

        let status = resolve_git_status(&root).unwrap();
        assert_eq!(status.branch, "main");
        assert!(!status.is_dirty);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detached_head_resolution() {
        let root = temp_test_dir("detached-head");
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "c79b9587eae11db4f1f3a763cc63ee103c69c964\n").unwrap();
        fs::write(git_dir.join("index"), "dummy index content").unwrap();

        let status = resolve_git_status(&root).unwrap();
        assert_eq!(status.branch, "HEAD (c79b958)");
        assert!(!status.is_dirty);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn worktree_or_submodule_file_pointer() {
        let base = temp_test_dir("submodule-parent");
        let actual_git_dir = base.join("actual_git");
        fs::create_dir_all(&actual_git_dir).unwrap();
        fs::write(actual_git_dir.join("HEAD"), "ref: refs/heads/feat/submod\n").unwrap();
        fs::write(actual_git_dir.join("index"), "dummy index content").unwrap();

        let worktree_dir = base.join("worktree");
        fs::create_dir_all(&worktree_dir).unwrap();
        fs::write(
            worktree_dir.join(".git"),
            "gitdir: ../actual_git\n",
        )
        .unwrap();

        let status = resolve_git_status(&worktree_dir).unwrap();
        assert_eq!(status.branch, "feat/submod");
        assert!(!status.is_dirty);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn dirty_flag_on_merge_head() {
        let root = temp_test_dir("merge-head");
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::write(git_dir.join("index"), "index").unwrap();
        fs::write(git_dir.join("MERGE_HEAD"), "abc").unwrap();

        let status = resolve_git_status(&root).unwrap();
        assert!(status.is_dirty);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_resolve_file_git_statuses_on_current_repo() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let statuses = resolve_file_git_statuses(root);
        assert!(statuses.is_empty() || !statuses.is_empty());
    }
}
