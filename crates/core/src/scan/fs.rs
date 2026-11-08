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
//! - **Session IO:** last-session state is read once at worker start
//!   (shipped as `Restored`) and written from the worker with a debounce
//!   (`Persist`). The frame never touches the state file.
//!
//! std-only except `notify` for the watcher.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use super::{DirSource, Listing, sort_entries};
use crate::model::{AppOpener, Entry, EntryKind, EntryStats, GitStatus};
use crate::session::{self, SessionState};

/// Paint this many entries immediately, unsorted, while the rest reads.
const FIRST_BATCH: usize = 96;
/// Cap on subdirectories warmed after a completed listing.
const WARM_BUDGET: usize = 16;
/// Cache cap (entries = full directory listings). Blunt clear keeps it honest.
const CACHE_CAP: usize = 256;
/// Debounce: filesystem dirtiness flushes after this quiet window.
const WATCH_QUIET: Duration = Duration::from_millis(150);
/// Worker loop cadence for watcher drain + flush.
const WATCH_POLL: Duration = Duration::from_millis(60);
/// Debounce: session writes coalesce after this quiet window.
const PERSIST_QUIET: Duration = Duration::from_millis(300);

pub(crate) enum Work {
    Scan {
        ticket: u64,
        dir: PathBuf,
    },
    Warm {
        dir: PathBuf,
    },
    /// Set the watch target (native backend: FSEvents/inotify/Windows).
    /// `dir` is the scan key the UI knows; canonicalization happens here.
    Watch {
        dir: PathBuf,
    },
    /// Store the session state; the worker debounces and writes it.
    Persist {
        state: SessionState,
    },
    /// Request file size and modification timestamp off-thread for visible rows.
    Stat {
        ticket: u64,
        paths: Vec<PathBuf>,
    },
    /// Enumerate candidate application openers for a given file.
    EnumerateOpeners {
        ticket: u64,
        path: PathBuf,
    },
    /// Calculate shallow directory size off-thread.
    StatDir {
        ticket: u64,
        dir: PathBuf,
    },
    /// Discover git branch and status off-thread.
    GitStatus {
        ticket: u64,
        dir: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatcherState {
    Active,
    Coalescing,
    Degraded,
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
    /// Filesystem changed under a watched directory; debounced by the
    /// worker on its quiet window. `dir` is the scan key the UI knows.
    Dirty {
        dir: PathBuf,
    },
    /// Sent once at worker start: the restored last-session state, or the
    /// default home state when no usable session file exists.
    Restored {
        state: SessionState,
    },
    /// Off-thread lazy stat results for requested paths.
    Stats {
        ticket: u64,
        batch: Vec<(PathBuf, EntryStats)>,
    },
    /// Candidate application openers enumerated for a path.
    Openers {
        ticket: u64,
        path: PathBuf,
        apps: Vec<AppOpener>,
    },
    /// Watcher quiet window active: coalescing filesystem events.
    Coalescing {
        dir: PathBuf,
    },
    /// Shallow directory size total bytes.
    DirSize {
        ticket: u64,
        total_bytes: u64,
    },
    /// Git repository status delivered.
    Git {
        ticket: u64,
        status: Option<GitStatus>,
    },
    /// Watcher initialization or registration failed.
    WatchDegraded {
        dir: PathBuf,
    },
}

pub type RepaintWaker = Arc<dyn Fn() + Send + Sync>;

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

fn spawn_worker(
    show_hidden: bool,
    session_path: Option<PathBuf>,
    waker: Option<RepaintWaker>,
    current_ticket: Arc<AtomicU64>,
) -> (mpsc::Sender<Work>, mpsc::Receiver<ScanEvent>) {
    let (work_tx, work_rx) = mpsc::channel::<Work>();
    let (event_tx, event_rx) = mpsc::channel::<ScanEvent>();

    thread::Builder::new()
        .name("ff-scan".into())
        .spawn(move || {
            let send_event = {
                let event_tx = event_tx;
                let waker = waker;
                move |event: ScanEvent| {
                    let res = event_tx.send(event);
                    if let Some(w) = &waker {
                        w();
                    }
                    res
                }
            };

            // Session restore is pure worker IO: the state file is read
            // here, and exactly one `Restored` event ships at start (with
            // the default home state when there is nothing usable).
            let restored_state = session_path
                .as_deref()
                .and_then(session::load)
                .unwrap_or_else(session::default_state);
            let _ = send_event(ScanEvent::Restored {
                state: restored_state,
            });

            // The native filesystem backend (FSEvents / inotify /
            // ReadDirectoryChangesW) lives wholly on this thread: watcher
            // creation, canonicalization, un/watch work, event handling.
            let mut watcher: Option<(
                RecommendedWatcher,
                mpsc::Receiver<notify::Result<notify::Event>>,
            )> = None;
            // (scan key the UI knows, canonical watch target)
            let mut watched: Option<(PathBuf, PathBuf)> = None;
            // scan key -> Instant of the latest dirtiness; flushes when the
            // quiet window elapses so event storms cannot cause scan storms.
            let mut dirty_since: HashMap<PathBuf, Instant> = HashMap::new();
            // (latest state, write-after Instant): re-set on every Persist,
            // so a burst of user activity coalesces into one write.
            let mut pending_persist: Option<(SessionState, Instant)> = None;
            let mut active_ticket: u64 = 0;

            loop {
                // A. Drain watcher events (non-blocking), coalesce.
                let watch_target = watched.clone();
                if watch_target.is_some()
                    && let Some((_, rx)) = watcher.as_ref() {
                        while let Ok(received) = rx.try_recv() {
                            let Ok(event) = received else {
                                continue;
                            };
                            for path in event.paths {
                                let Some(event_dir) = path.parent() else {
                                    continue;
                                };
                                let Ok(event_dir) = std::fs::canonicalize(event_dir) else {
                                    continue;
                                };
                                if let Some((scan_key, canonical)) = watch_target.as_ref()
                                    && event_dir == *canonical {
                                        let is_first = !dirty_since.contains_key(scan_key);
                                        dirty_since.insert(scan_key.clone(), Instant::now());
                                        if is_first {
                                            let _ = send_event(ScanEvent::Coalescing {
                                                dir: scan_key.clone(),
                                            });
                                        }
                                    }
                            }
                        }
                    }

                // B. Flush quiet windows: dirty directories (as Dirt events)
                //    and debounced session writes.
                let now = Instant::now();
                let flush: Vec<PathBuf> = dirty_since
                    .iter()
                    .filter(|(_, since)| now.duration_since(**since) >= WATCH_QUIET)
                    .map(|(key, _)| key.clone())
                    .collect();
                for dir in flush {
                    dirty_since.remove(&dir);
                    let _ = send_event(ScanEvent::Dirty { dir });
                }
                if let Some((state, at)) = &mut pending_persist
                    && now >= *at {
                        if let Some(path) = &session_path {
                            let _ = session::save(path, state);
                        }
                        pending_persist = None;
                    }

                // C. Accept work; watcher drains happen each loop rotation.
                match work_rx.recv_timeout(WATCH_POLL) {
                    Ok(work) => match work {
                        Work::Scan { ticket, dir } => {
                            active_ticket = ticket;
                            let mut send_batch = |batch: Vec<Entry>| {
                                let _ = send_event(ScanEvent::Batch {
                                    ticket,
                                    entries: batch,
                                });
                            };
                            match read_entries(&dir, show_hidden, &mut send_batch) {
                                Ok(mut entries) => {
                                    sort_entries(&mut entries);
                                    let _ = send_event(ScanEvent::Ready {
                                        ticket,
                                        dir,
                                        entries,
                                    });
                                }
                                Err(e) => {
                                    let _ = send_event(ScanEvent::Failed {
                                        ticket,
                                        reason: e.to_string(),
                                    });
                                }
                            }
                        }
                        Work::Warm { dir } => {
                            let mut send_batch = |_: Vec<Entry>| {}; // warm fills are batch-less
                            if let Ok(mut entries) =
                                read_entries(&dir, show_hidden, &mut send_batch)
                            {
                                sort_entries(&mut entries);
                                let _ = send_event(ScanEvent::Ready {
                                    ticket: u64::MAX, // warm fills carry no live ticket
                                    dir,
                                    entries,
                                });
                            }
                        }
                        Work::Persist { state } => {
                            // Coalesced: each successive state replaces the
                            // pending one and restarts the quiet window.
                            pending_persist = Some((state, Instant::now() + PERSIST_QUIET));
                        }
                        Work::Watch { dir } => {
                            // Tear down the previous target so watchers never
                            // accumulate across navigation.
                            if let Some((_, old_canonical)) = watched.take()
                                && let Some((w, _)) = watcher.as_mut() {
                                    let _ = w.unwatch(&old_canonical);
                                }
                            // A failed canonicalize just means no
                            // auto-refresh for this dir; scanning still works.
                            match std::fs::canonicalize(&dir) {
                                Ok(canonical) => {
                                    let (watch_tx, watch_rx) = mpsc::channel();
                                    match notify::recommended_watcher(watch_tx) {
                                        Ok(mut w) => {
                                            if w.watch(&canonical, RecursiveMode::NonRecursive)
                                                .is_ok()
                                            {
                                                // Catch anything that happened
                                                // between scan and watch setup.
                                                dirty_since.insert(dir.clone(), Instant::now());
                                                watcher = Some((w, watch_rx));
                                                watched = Some((dir, canonical));
                                            } else {
                                                watcher = None;
                                                watched = None;
                                                let _ = send_event(ScanEvent::WatchDegraded { dir });
                                            }
                                        }
                                        Err(_) => {
                                            watcher = None;
                                            watched = None;
                                            let _ = send_event(ScanEvent::WatchDegraded { dir });
                                        }
                                    }
                                }
                                Err(_) => {
                                    watcher = None;
                                    watched = None;
                                    let _ = send_event(ScanEvent::WatchDegraded { dir });
                                }
                            }
                        }
                        Work::Stat { ticket, paths } => {
                            // Monotonic ticket fencing: drop stale requests immediately
                            if ticket == active_ticket {
                                let mut batch = Vec::with_capacity(paths.len());
                                for path in paths {
                                    let stats = match std::fs::symlink_metadata(&path) {
                                        Ok(meta) => {
                                            let size = meta.len();
                                            let mtime_secs = meta
                                                .modified()
                                                .ok()
                                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                                .map(|d| d.as_secs() as i64)
                                                .unwrap_or(0);
                                            EntryStats { size, mtime_secs }
                                        }
                                        Err(_) => EntryStats::default(),
                                    };
                                    batch.push((path, stats));
                                }
                                let _ = send_event(ScanEvent::Stats { ticket, batch });
                            }
                        }
                        Work::EnumerateOpeners { ticket, path } => {
                            if ticket == active_ticket {
                                let apps = crate::ops::open::enumerate_openers(&path);
                                let _ = send_event(ScanEvent::Openers { ticket, path, apps });
                            }
                        }
                        Work::StatDir { ticket, dir } => {
                            if ticket == current_ticket.load(Ordering::Relaxed) {
                                let mut total_bytes: u64 = 0;
                                let mut count: usize = 0;
                                if let Ok(entries) = std::fs::read_dir(&dir) {
                                    for entry in entries {
                                        count += 1;
                                        if count.is_multiple_of(256)
                                            && (current_ticket.load(Ordering::Relaxed) != ticket || count >= 10_000)
                                        {
                                            break;
                                        }
                                        if let Ok(entry) = entry
                                            && let Ok(file_type) = entry.file_type()
                                            && file_type.is_file()
                                            && let Ok(meta) = std::fs::symlink_metadata(entry.path())
                                        {
                                            total_bytes = total_bytes.saturating_add(meta.len());
                                        }
                                    }
                                }
                                if current_ticket.load(Ordering::Relaxed) == ticket {
                                    let _ = send_event(ScanEvent::DirSize { ticket, total_bytes });
                                }
                            }
                        }
                        Work::GitStatus { ticket, dir } => {
                            if ticket == current_ticket.load(Ordering::Relaxed) {
                                let status = crate::ops::git::resolve_git_status(&dir);
                                if current_ticket.load(Ordering::Relaxed) == ticket {
                                    let _ = send_event(ScanEvent::Git { ticket, status });
                                }
                            }
                        }
                    },
                    Err(mpsc::RecvTimeoutError::Timeout) => {} // drains happen next loop
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
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
    current_ticket: Arc<AtomicU64>,
    live_dir: PathBuf,
    live_entries: Vec<Entry>,
    /// Authoritative listing flag: false while loading/streaming, true after
    /// Ready (or cache hit) for the live dir.
    ready: bool,
    queued: bool,
    cache: HashMap<PathBuf, Vec<Entry>>,
    warms_inflight: HashSet<PathBuf>,
    error: Option<String>,
    /// Restored session state, buffered one-shot until the shell consumes it.
    /// The worker always ships exactly one `Restored` at start, so `take`
    /// returning `None` simply means "not yet arrived / already applied".
    restored: Option<SessionState>,
    /// Off-thread stat batches delivered from the worker, buffered until taken.
    pending_stats: Vec<(PathBuf, EntryStats)>,
    /// Off-thread application opener query result.
    pending_openers: Option<(PathBuf, Vec<AppOpener>)>,
    /// Off-thread shallow directory byte size total.
    dir_size: Option<u64>,
    /// Off-thread git repository branch and dirty status.
    git_status: Option<GitStatus>,
    /// Watcher health / coalescing status.
    watcher_state: WatcherState,
}

impl FsEngine {
    pub fn launch(show_hidden: bool) -> Self {
        Self::launch_with_options(show_hidden, None, None)
    }

    /// `session_path` points at the last-session state file (`None`
    /// disables session persistence — used by tests and degraded setups).
    pub fn launch_with(show_hidden: bool, session_path: Option<PathBuf>) -> Self {
        Self::launch_with_options(show_hidden, session_path, None)
    }

    pub fn with_waker(session_path: Option<PathBuf>, waker: RepaintWaker) -> Self {
        Self::launch_with_options(false, session_path, Some(waker))
    }

    pub fn launch_with_waker(
        show_hidden: bool,
        session_path: Option<PathBuf>,
        waker: Option<RepaintWaker>,
    ) -> Self {
        Self::launch_with_options(show_hidden, session_path, waker)
    }

    pub fn launch_with_options(
        show_hidden: bool,
        session_path: Option<PathBuf>,
        waker: Option<RepaintWaker>,
    ) -> Self {
        let current_ticket = Arc::new(AtomicU64::new(0));
        let (work_tx, events) = spawn_worker(show_hidden, session_path, waker, current_ticket.clone());
        Self {
            work_tx,
            events,
            ticket: 0,
            current_ticket,
            live_dir: PathBuf::new(),
            live_entries: Vec::new(),
            ready: false,
            queued: false,
            cache: HashMap::new(),
            warms_inflight: HashSet::new(),
            error: None,
            restored: None,
            pending_stats: Vec::new(),
            pending_openers: None,
            dir_size: None,
            git_status: None,
            watcher_state: WatcherState::Active,
        }
    }

    pub fn last_error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn dir_size(&self) -> Option<u64> {
        self.dir_size
    }

    pub fn git_status(&self) -> Option<&GitStatus> {
        self.git_status.as_ref()
    }

    pub fn watcher_state(&self) -> WatcherState {
        self.watcher_state
    }

    /// Consumes the restored session state once it has arrived (the shell
    /// applies it: cwd, selection anchor, scroll request).
    pub fn take_restored(&mut self) -> Option<SessionState> {
        self.restored.take()
    }

    /// Consumes any pending stat batches delivered by the background worker.
    pub fn take_stats(&mut self) -> Vec<(PathBuf, EntryStats)> {
        std::mem::take(&mut self.pending_stats)
    }

    /// Consumes any pending application opener query results delivered by the worker.
    pub fn take_openers(&mut self) -> Option<(PathBuf, Vec<AppOpener>)> {
        self.pending_openers.take()
    }

    /// Enqueues an off-thread application opener enumeration query stamped with current ticket.
    pub fn enumerate_openers(&self, path: PathBuf) {
        let _ = self.work_tx.send(Work::EnumerateOpeners {
            ticket: self.ticket,
            path,
        });
    }

    /// Enqueues an off-thread stat request stamped with the current navigation ticket.
    pub fn request_stats(&self, paths: Vec<PathBuf>) {
        if !paths.is_empty() {
            let _ = self.work_tx.send(Work::Stat {
                ticket: self.ticket,
                paths,
            });
        }
    }

    /// Hands the current session state to the worker thread; writing is
    /// debounced there (PERSIST_QUIET), so bursts of activity coalesce.
    pub fn persist(&self, state: SessionState) {
        let _ = self.work_tx.send(Work::Persist { state });
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
                        if self.watcher_state == WatcherState::Coalescing {
                            self.watcher_state = WatcherState::Active;
                        }
                    }
                }
                ScanEvent::Failed { ticket, reason, .. } => {
                    if ticket == self.ticket {
                        self.error = Some(reason);
                        self.queued = false;
                    }
                }
                ScanEvent::Restored { state } => {
                    self.restored = Some(state);
                }
                ScanEvent::Dirty { dir } => {
                    // Live dir dirtiness: force a fresh authoritative listing
                    // on the next poll. The anchor keeps the user's selection
                    // stable across the refresh; the old list stays visible
                    // meanwhile instead of flashing empty.
                    self.cache.remove(&dir);
                    self.pending_stats.retain(|(p, _)| p.parent() != Some(&dir));
                    if dir == self.live_dir {
                        self.ready = false;
                        self.queued = false;
                        self.error = None;
                        let _ = self.work_tx.send(Work::StatDir {
                            ticket: self.ticket,
                            dir: dir.clone(),
                        });
                        let _ = self.work_tx.send(Work::GitStatus {
                            ticket: self.ticket,
                            dir,
                        });
                    }
                }
                ScanEvent::Stats { ticket, batch } if ticket == self.ticket => {
                    self.pending_stats.extend(batch);
                }
                ScanEvent::Openers { ticket, path, apps } if ticket == self.ticket => {
                    self.pending_openers = Some((path, apps));
                }
                ScanEvent::Coalescing { dir } => {
                    if dir == self.live_dir {
                        self.watcher_state = WatcherState::Coalescing;
                    }
                }
                ScanEvent::DirSize { ticket, total_bytes } if ticket == self.ticket => {
                    self.dir_size = Some(total_bytes);
                }
                ScanEvent::Git { ticket, status } if ticket == self.ticket => {
                    self.git_status = status;
                }
                ScanEvent::WatchDegraded { dir } if dir == self.live_dir => {
                    self.watcher_state = WatcherState::Degraded;
                }
                _ => {} // stale ticket, dropped
            }
        }

        // Placeholder: an empty path means "not navigated yet" — the shell
        // parks here until the restored (or default home) state names a real
        // directory. Never enqueue scans or watches for nothing.
        if dir.as_os_str().is_empty() {
            return Listing::default();
        }

        // 2. Directory switch: reset the live scheme under a fresh ticket,
        //    and move the native watch target onto the new directory.
        if dir != self.live_dir {
            self.ticket += 1;
            self.current_ticket.store(self.ticket, Ordering::Relaxed);
            self.live_dir = dir.to_path_buf();
            self.live_entries.clear();
            self.ready = false;
            self.queued = false;
            self.error = None;
            self.pending_stats.clear();
            self.pending_openers = None;
            self.dir_size = None;
            self.git_status = None;
            self.watcher_state = WatcherState::Active;
            let _ = self.work_tx.send(Work::Watch {
                dir: dir.to_path_buf(),
            });
            let _ = self.work_tx.send(Work::StatDir {
                ticket: self.ticket,
                dir: dir.to_path_buf(),
            });
            let _ = self.work_tx.send(Work::GitStatus {
                ticket: self.ticket,
                dir: dir.to_path_buf(),
            });
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
            let _ = engine.poll(&missing);
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

    #[test]
    fn watcher_refreshes_listing_on_change() {
        let root = temp_root("watch");
        fs::write(root.join("a.txt"), "").unwrap();

        let mut engine = FsEngine::launch(false);
        // Primes the listing AND moves the watch target onto `root`.
        let _ = wait_complete(&mut engine, &root);

        // Change the tree while watched; the engine must pick it up via
        // the debounced Dirty event and produce a fresh complete listing.
        fs::create_dir(root.join("newdir")).unwrap();

        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let listing = engine.poll(&root);
            if listing.entries.iter().any(|e| e.name == "newdir") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "watch-driven refresh did not arrive in time"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            engine.last_error().is_none(),
            "watch refresh must not produce errors"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_ships_default_when_no_session_file() {
        let mut engine = FsEngine::launch(false);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let _ = engine.poll(Path::new(""));
            if let Some(state) = engine.take_restored() {
                assert_eq!(state, session::default_state());
                break;
            }
            assert!(Instant::now() < deadline, "restore event missed");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn restore_reads_last_session_and_worker_persists_changes() {
        let data = temp_root("session-data");
        let listed = temp_root("session-dir");
        fs::write(listed.join("a.txt"), "").unwrap();
        fs::write(listed.join("b.txt"), "").unwrap();
        let state_path = data.join("session");
        session::save(
            &state_path,
            &SessionState {
                cwd: listed.clone(),
                selection: Some("b.txt".into()),
                frequents: Vec::new(),
            },
        )
        .unwrap();

        let mut engine = FsEngine::launch_with(false, Some(state_path.clone()));
        let deadline = Instant::now() + Duration::from_secs(5);
        let restored = loop {
            let _ = engine.poll(Path::new(""));
            if let Some(state) = engine.take_restored() {
                break state;
            }
            assert!(Instant::now() < deadline, "restore not delivered in time");
            thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(
            restored,
            SessionState {
                cwd: listed.clone(),
                selection: Some("b.txt".into()),
                frequents: Vec::new(),
            }
        );

        // Navigating to the restored dir shows its contents.
        let listing = wait_complete(&mut engine, &listed);
        assert!(listing.entries.iter().any(|e| e.name == "b.txt"));

        // User activity persists through the worker (debounced write).
        let next = SessionState {
            cwd: listed.clone(),
            selection: Some("a.txt".into()),
            frequents: Vec::new(),
        };
        engine.persist(next.clone());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if session::load(&state_path) == Some(next.clone()) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "persist write not flushed in time"
            );
            thread::sleep(Duration::from_millis(10));
        }

        let _ = fs::remove_dir_all(&data);
        let _ = fs::remove_dir_all(&listed);
    }

    #[test]
    fn waker_is_invoked_on_events() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let wake_count = Arc::new(AtomicUsize::new(0));
        let count_clone = Arc::clone(&wake_count);
        let waker = Arc::new(move || {
            count_clone.fetch_add(1, Ordering::SeqCst);
        });

        let mut engine = FsEngine::launch_with_waker(false, None, Some(waker));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let _ = engine.poll(Path::new(""));
            if engine.take_restored().is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "timed out waiting for restore event");
            thread::sleep(Duration::from_millis(2));
        }

        assert!(wake_count.load(Ordering::SeqCst) > 0);
    }

    #[test]
    fn stats_pipeline_delivers_file_sizes_and_mtimes() {
        let root = temp_root("stat-pipeline");
        let file_a = root.join("alpha.txt");
        let file_b = root.join("beta.bin");
        fs::write(&file_a, "hello world").unwrap();
        fs::write(&file_b, vec![0u8; 2048]).unwrap();

        let mut engine = FsEngine::launch(false);
        let _ = wait_complete(&mut engine, &root);

        engine.request_stats(vec![file_a.clone(), file_b.clone()]);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stats_map = HashMap::new();
        loop {
            let _ = engine.poll(&root);
            for (path, s) in engine.take_stats() {
                stats_map.insert(path, s);
            }
            if stats_map.contains_key(&file_a) && stats_map.contains_key(&file_b) {
                break;
            }
            assert!(Instant::now() < deadline, "stat delivery timed out");
            thread::sleep(Duration::from_millis(5));
        }

        assert_eq!(stats_map[&file_a].size, 11);
        assert_eq!(stats_map[&file_b].size, 2048);
        assert!(stats_map[&file_a].mtime_secs > 0);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn openers_pipeline_delivers_applications() {
        let root = temp_root("opener-pipeline");
        let file = root.join("test.txt");
        fs::write(&file, "hello").unwrap();

        let mut engine = FsEngine::launch(false);
        let _ = wait_complete(&mut engine, &root);

        engine.enumerate_openers(file.clone());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let _ = engine.poll(&root);
            if let Some((path, apps)) = engine.take_openers() {
                assert_eq!(path, file);
                #[cfg(target_os = "macos")]
                assert!(!apps.is_empty());
                let _ = apps;
                break;
            }
            assert!(Instant::now() < deadline, "opener query timed out");
            thread::sleep(Duration::from_millis(5));
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dir_size_pipeline_delivers_shallow_total_bytes() {
        let root = temp_root("dir-size-pipeline");
        fs::write(root.join("f1.txt"), "12345").unwrap(); // 5 bytes
        fs::write(root.join("f2.txt"), "1234567890").unwrap(); // 10 bytes
        let sub = root.join("subdir");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("nested.txt"), "nested content that should not be counted").unwrap();

        let mut engine = FsEngine::launch(false);
        let _ = wait_complete(&mut engine, &root);

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let _ = engine.poll(&root);
            if let Some(size) = engine.dir_size() {
                assert_eq!(size, 15);
                break;
            }
            assert!(Instant::now() < deadline, "dir size timed out");
            thread::sleep(Duration::from_millis(5));
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn git_status_pipeline_delivers_branch_and_status() {
        let root = temp_root("git-status-pipeline");
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/feature/hairline\n").unwrap();
        fs::write(git_dir.join("index"), "dummy index content").unwrap();

        let mut engine = FsEngine::launch(false);
        let _ = wait_complete(&mut engine, &root);

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let _ = engine.poll(&root);
            if let Some(status) = engine.git_status() {
                assert_eq!(status.branch, "feature/hairline");
                assert!(!status.is_dirty);
                break;
            }
            assert!(Instant::now() < deadline, "git status timed out");
            thread::sleep(Duration::from_millis(5));
        }

        let _ = fs::remove_dir_all(&root);
    }
}
