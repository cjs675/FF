//! In-memory undo journal tracking reversible file operations (PLAN §4 ops).

use std::path::PathBuf;

/// Reversible file actions managed by the undo journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoAction {
    /// Restore a trashed item from its trash location back to its original path.
    RestoreTrash {
        original_path: PathBuf,
        trash_path: Option<PathBuf>,
    },
    /// Re-rename an item from `current` back to `original`.
    Rename {
        current: PathBuf,
        original: PathBuf,
    },
    /// Move a created file to trash to undo its creation.
    DeleteCreatedFile {
        path: PathBuf,
    },
    /// Remove an empty created directory to undo its creation.
    DeleteCreatedDir {
        path: PathBuf,
    },
}

/// An entry in the undo journal with a user-facing description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoEntry {
    pub description: String,
    pub action: UndoAction,
}

/// Bounded in-memory stack of reversible operations.
#[derive(Debug)]
pub struct UndoJournal {
    entries: Vec<UndoEntry>,
    max_entries: usize,
}

impl Default for UndoJournal {
    fn default() -> Self {
        Self::new(50)
    }
}

impl UndoJournal {
    /// Create a new undo journal with a maximum entry depth.
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::with_capacity(max_entries),
            max_entries: max_entries.max(1),
        }
    }

    /// Record a reversible action.
    pub fn record(&mut self, description: impl Into<String>, action: UndoAction) {
        if self.entries.len() >= self.max_entries {
            self.entries.remove(0);
        }
        self.entries.push(UndoEntry {
            description: description.into(),
            action,
        });
    }

    /// Pop the most recent action to undo.
    pub fn pop(&mut self) -> Option<UndoEntry> {
        self.entries.pop()
    }

    /// Peek at the most recent action without removing it.
    pub fn peek(&self) -> Option<&UndoEntry> {
        self.entries.last()
    }

    /// Check if the journal is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Current number of recorded actions.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Clear all journal history.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_journal_push_pop_and_bounds() {
        let mut journal = UndoJournal::new(2);
        assert!(journal.is_empty());
        assert_eq!(journal.pop(), None);

        journal.record("file 1", UndoAction::DeleteCreatedFile { path: PathBuf::from("a") });
        journal.record("file 2", UndoAction::DeleteCreatedFile { path: PathBuf::from("b") });
        journal.record("file 3", UndoAction::DeleteCreatedFile { path: PathBuf::from("c") });

        // Cap was 2, so file 1 was evicted
        assert_eq!(journal.len(), 2);

        let top = journal.pop().unwrap();
        assert_eq!(top.description, "file 3");

        let second = journal.pop().unwrap();
        assert_eq!(second.description, "file 2");

        assert!(journal.is_empty());
    }
}
