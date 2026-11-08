//! Fast, non-blocking git metadata resolution (PLAN §10 it16).
//! Discovers git root, handles submodules/worktrees, resolves branches/detached HEAD,
//! and evaluates fast index dirtiness without launching child processes.

use std::path::{Path, PathBuf};
use crate::model::GitStatus;

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
}
