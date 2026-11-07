//! The real engine: one worker thread owns all filesystem reads (PLAN §2).
//!
//! - **No-stat contract:** enumeration uses `read_dir` + `file_type()`
//!   (`d_type` off the dirents). `metadata()` is never called per entry —
//!   columns stat lazily in a later milestone.
//! - **Viewport-first:** the first chunk paints as an immediate `Batch`
//!   event (raw order), the complete sorted list arrives as `Ready`.
//! - **Tickets:** every scheme switch bumps the ticket; events older than
//!   the current ticket are dropped, so fast navigation never mixes trees.
//! - **Neighbor warm cache:** completed listings warm their subdirectories
//!   (bounded) so descending hits an instant cached listing.
//!
//! Everything here is std-only by design; `notify` rides in with the
//! watcher milestone.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use super::{DirSource, Listing, sort_entries};
use crate::model::{Entry, EntryKind};

/// Paint this many entries immediately, unsorted, while the rest reads.
const FIRST_BATCH: usize = 96;
/// Cap on subdirectories warmed after a completed listing.
const WARM_BUDGET: usize = 16;
/// Cache cap (entries = full directory listings). Blunt clear keeps it honest.
const CACHE_CAP: usize = 256;

pub(crate) enum Work {
    Scan { ticket: u64, dir: PathBuf },
    Warm { dir: PathBuf },
}

pub(crate) enum ScanEvent {
    /// Immediate raw prefix for viewport-first painting.
    Batch {
        ticket: u64,
        entries: Vec<Entry>,
    },
    /// Complete sorted listing for `dir` (from a Scan with a live ticket,
    /// or a Warm fill with no ticket).
    Ready {
        ticket: u64,
        dir: PathBuf,
        entries: Vec<Entry>,
    },
    Failed {
        ticket: u64,
        reason: String,
    },
}

fn kind_of(t: fs::FileType) -> EntryKind {
    if t.is_dir() {
        EntryKind::Dir
    } else if t.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::File
    }
}

/// Lists `dir` with the no-stat contract, streaming the first batch through
/// `send_batch` as soon as it exists. Full result is returned sorted.
fn read_entries(
    dir: &Path,
    show_hidden: bool,
    send_batch: &mut dyn FnMut(Vec<Entry>),
) -> io::Result<Vec<Entry>> {
    let mut all: Vec<Entry> = Vec::new();
    let mut batch: Vec<Entry> = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue, // one bad entry must not kill the listing
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        // file_type() reads d_type from the dirent — no metadata() call.
        let kind = match entry.file_type() {
            Ok(t) => kind_of(t),
            Err(_) => continue,
        };
        let path = dir.join(&name);
        batch.push(Entry { name, path, kind });
        if batch.len() >= FIRST_BATCH {
            let drained = std::mem::take(&mut batch);
            send_batch(drained);
        }
    }
    all.append(&mut batch);
    Ok(all)
}

fn spawn_worker(show_hidden: bool) -> (mpsc::Sender<Work>, mpsc::Receiver<ScanEvent>) {
    let (work_tx, work_rx) = mpsc::channel::<Work>();
    let (event_tx, event_rx) = mpsc::channel::<ScanEvent>();

    thread::Builder::new()
        .name("ff-scan".into())
        .spawn(move || {
            while let Ok(work) = work_rx.recv() {
                match work {
                    Work::Scan { ticket, dir } => {
                        let mut send_batch = |batch: Vec<Entry>| {
                            let _ = event_tx.send(ScanEvent::Batch {
                                ticket,
                                entries: batch,
                            });
                        };
                        match read_entries(&dir, show_hidden, &mut send_batch) {
                            Ok(mut entries) => {
                                sort_entries(&mut entries);
                                let _ = event_tx.send(ScanEvent::Ready {
                                    ticket,
                                    dir,
                                    entries,
                                });
                            }
                            Err(e) => {
                                let _ = event_tx.send(ScanEvent::Failed {
                                    ticket,
                                    reason: e.to_string(),
                                });
                            }
                        }
                    }
                    Work::Warm { dir } => {
                        let mut send_batch = |_: Vec<Entry>| {}; // warm fills are batch-less
                        if let Ok(mut entries) = read_entries(&dir, show_hidden, &mut send_batch) {
                            sort_entries(&mut entries);
                            let _ = event_tx.send(ScanEvent::Ready {
                                ticket: u64::MAX, // warm fills carry no live ticket
                                dir,
                                entries,
                            });
                        }
                    }
                }
            }
        })
        .expect("spawn ff-scan worker");

    (work_tx, event_rx)
}

/// UI-side engine. Owns the event channel; `poll` drains, reconciles, and
/// returns the best-known listing without ever touching the filesystem.
pub struct FsEngine {
    work_tx: mpsc::Sender<Work>,
    events: mpsc::Receiver<ScanEvent>,
    ticket: u64,
    live_dir: PathBuf,
    live_entries: Vec<Entry>,
    /// Authoritative listing flag: false while loading/streaming, true after
    /// Ready (or cache hit) for the live dir.
    ready: bool,
    queued: bool,
    cache: HashMap<PathBuf, Vec<Entry>>,
    warms_inflight: HashSet<PathBuf>,
    error: Option<String>,
}

impl FsEngine {
    pub fn launch(show_hidden: bool) -> Self {
        let (work_tx, events) = spawn_worker(show_hidden);
        Self {
            work_tx,
            events,
            ticket: 0,
            live_dir: PathBuf::new(),
            live_entries: Vec::new(),
            ready: false,
            queued: false,
            cache: HashMap::new(),
            warms_inflight: HashSet::new(),
            error: None,
        }
    }

    pub fn last_error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn enqueue_warm(&mut self, dir: PathBuf) {
        if self.cache.contains_key(&dir) || self.warms_inflight.contains(&dir) {
            return;
        }
        self.warms_inflight.insert(dir.clone());
        let _ = self.work_tx.send(Work::Warm { dir });
    }
}

impl DirSource for FsEngine {
    fn poll(&mut self, dir: &Path) -> Listing {
        // 1. Drain non-blocking events; tickets fence stale results.
        while let Ok(event) = self.events.try_recv() {
            match event {
                ScanEvent::Batch { ticket, entries } if ticket == self.ticket => {
                    self.live_entries.extend(entries);
                    self.ready = false;
                }
                ScanEvent::Ready {
                    ticket,
                    dir,
                    entries,
                } => {
                    self.error = None;
                    if self.cache.len() > CACHE_CAP {
                        self.cache.clear();
                    }
                    self.warms_inflight.remove(&dir);
                    self.cache.insert(dir.clone(), entries.clone());
                    if ticket == self.ticket && dir == self.live_dir {
                        self.live_entries = entries;
                        self.ready = true;
                        self.queued = false;
                    }
                }
                ScanEvent::Failed { ticket, reason, .. } => {
                    if ticket == self.ticket {
                        self.error = Some(reason);
                        self.queued = false;
                    }
                }
                _ => {} // stale ticket, dropped
            }
        }

        // 2. Directory switch: reset the live scheme under a fresh ticket.
        if dir != self.live_dir {
            self.ticket += 1;
            self.live_dir = dir.to_path_buf();
            self.live_entries.clear();
            self.ready = false;
            self.queued = false;
            self.error = None;
        }

        // 3. Best-known listing: cache hit (instant) or in-flight partial.
        if let Some(ready) = self.cache.get(dir) {
            self.live_entries = ready.clone();
            self.ready = true;
        } else if !self.queued && self.error.is_none() {
            self.queued = true;
            let _ = self.work_tx.send(Work::Scan {
                ticket: self.ticket,
                dir: dir.to_path_buf(),
            });
        }

        // 4. Neighbor prefetch: once this listing is complete, warm its
        //    subdirectories so descending costs nothing.
        if self.ready {
            let warm_candidates: Vec<PathBuf> = self
                .live_entries
                .iter()
                .filter(|e| e.is_dir())
                .take(WARM_BUDGET)
                .map(|e| e.path.clone())
                .collect();
            for dir in warm_candidates {
                self.enqueue_warm(dir);
            }
        }

        Listing {
            entries: self.live_entries.clone(),
            complete: self.ready,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ff-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn wait_complete(engine: &mut FsEngine, dir: &Path) -> Listing {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let listing = engine.poll(dir);
            if listing.complete {
                return listing;
            }
            assert!(Instant::now() < deadline, "engine did not complete in time");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn lists_tree_no_hidden_and_streaming_completes() {
        let root = temp_root("scan");
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("a.rs"), "").unwrap();
        fs::write(root.join("b.txt"), "").unwrap();
        fs::write(root.join(".hidden"), "").unwrap();

        let mut engine = FsEngine::launch(false);
        let listing = wait_complete(&mut engine, &root);

        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["sub", "a.rs", "b.txt"]);
        // second poll hits the cache: instant and complete
        let cached = engine.poll(&root);
        assert_eq!(cached.entries.len(), 3);
        assert!(cached.complete);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hidden_files_visible_when_enabled() {
        let root = temp_root("hidden");
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join(".dot"), "").unwrap();

        let mut engine = FsEngine::launch(true);
        let listing = wait_complete(&mut engine, &root);
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["sub", ".dot"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_dir_reports_failure_without_spamming() {
        let missing = std::env::temp_dir().join(format!("ff-test-missing-{}", std::process::id()));
        let mut engine = FsEngine::launch(false);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let listing = engine.poll(&missing);
            if engine.last_error().is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "failure not reported in time");
            thread::sleep(Duration::from_millis(2));
        }
        assert!(
            engine
                .last_error()
                .unwrap()
                .contains("No such file or directory")
        );
        let _after = engine.poll(&missing);
    }
}
