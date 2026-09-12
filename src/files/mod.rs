//! File resolution, Markdown discovery, and event-independent reload state.
//!
//! Scanning and polling contain no UI or watcher dependency. Callers can run
//! the synchronous APIs off the UI thread, or use [`DiscoveryScanner::spawn`].

use std::{
    fmt, fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::{Duration, SystemTime},
};

use crate::contracts::{
    DiscoveryBatch, DiscoveryComplete, DiscoveryEntry, DiscoveryError, Generation, MAX_BATCH_ITEMS,
    MAX_SOURCE_BYTES, RootId, ScanId,
};

/// Chosen save-burst debounce. Refresh latency is measured after this delay.
pub const RELOAD_DEBOUNCE: Duration = Duration::from_millis(75);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathKind {
    File,
    Directory,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedPath {
    pub path: PathBuf,
    pub kind: PathKind,
    pub explicit: bool,
}

#[derive(Debug, PartialEq)]
pub enum PathError {
    InvalidCallerCwd(PathBuf),
    Missing(PathBuf),
    Unreadable(PathBuf),
    NotAFile(PathBuf),
    NotADirectory(PathBuf),
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCallerCwd(path) => {
                write!(f, "caller cwd is not absolute: {}", path.display())
            }
            Self::Missing(path) => write!(f, "path does not exist: {}", path.display()),
            Self::Unreadable(path) => write!(f, "path is unreadable: {}", path.display()),
            Self::NotAFile(path) => write!(f, "path is not a file: {}", path.display()),
            Self::NotADirectory(path) => write!(f, "path is not a directory: {}", path.display()),
        }
    }
}

impl std::error::Error for PathError {}

/// Resolve launch input against caller cwd. No canonicalization is done here:
/// native policy remains responsible for symlink and authority checks.
pub fn resolve_path(
    caller_cwd: &Path,
    requested: Option<&Path>,
) -> Result<ResolvedPath, PathError> {
    if !caller_cwd.is_absolute() {
        return Err(PathError::InvalidCallerCwd(caller_cwd.to_owned()));
    }
    let explicit = requested.is_some();
    let path = requested.map_or_else(
        || caller_cwd.to_owned(),
        |value| {
            if value.is_absolute() {
                value.to_owned()
            } else {
                caller_cwd.join(value)
            }
        },
    );
    let metadata = fs::metadata(&path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => PathError::Missing(path.clone()),
        _ => PathError::Unreadable(path.clone()),
    })?;
    if metadata.is_file() {
        Ok(ResolvedPath {
            path,
            kind: PathKind::File,
            explicit,
        })
    } else if metadata.is_dir() {
        Ok(ResolvedPath {
            path,
            kind: PathKind::Directory,
            explicit,
        })
    } else {
        Err(PathError::Unreadable(path))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedSource {
    pub path: PathBuf,
    pub source: String,
    pub bytes: usize,
}

impl LoadedSource {
    pub fn is_empty(&self) -> bool {
        self.source.is_empty()
    }
}

#[derive(Debug)]
pub enum LoadError {
    Missing(PathBuf),
    Unreadable(PathBuf),
    NotAFile(PathBuf),
    InvalidUtf8(PathBuf),
    NeedsConfirmation { path: PathBuf, bytes: usize },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(path) => write!(f, "file does not exist: {}", path.display()),
            Self::Unreadable(path) => write!(f, "file is unreadable: {}", path.display()),
            Self::NotAFile(path) => write!(f, "path is not a file: {}", path.display()),
            Self::InvalidUtf8(path) => write!(f, "file is not UTF-8 Markdown: {}", path.display()),
            Self::NeedsConfirmation { path, bytes } => {
                write!(
                    f,
                    "file exceeds 10 MiB and needs confirmation ({} bytes): {}",
                    bytes,
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// Read complete source. Large files are never truncated.
pub fn load_source(path: &Path, confirmed_large_file: bool) -> Result<LoadedSource, LoadError> {
    let metadata = fs::metadata(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => LoadError::Missing(path.to_owned()),
        _ => LoadError::Unreadable(path.to_owned()),
    })?;
    if !metadata.is_file() {
        return Err(LoadError::NotAFile(path.to_owned()));
    }
    let bytes = metadata.len() as usize;
    if bytes > MAX_SOURCE_BYTES && !confirmed_large_file {
        return Err(LoadError::NeedsConfirmation {
            path: path.to_owned(),
            bytes,
        });
    }
    let raw = fs::read(path).map_err(|_| LoadError::Unreadable(path.to_owned()))?;
    let source = String::from_utf8(raw).map_err(|_| LoadError::InvalidUtf8(path.to_owned()))?;
    Ok(LoadedSource {
        path: path.to_owned(),
        bytes,
        source,
    })
}

#[derive(Clone, Debug)]
pub enum DiscoveryFailure {
    Missing(PathBuf),
    Unreadable(PathBuf),
    NotADirectory(PathBuf),
    Stale,
}

impl fmt::Display for DiscoveryFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(path) => write!(f, "discovery root does not exist: {}", path.display()),
            Self::Unreadable(path) => write!(f, "discovery root is unreadable: {}", path.display()),
            Self::NotADirectory(path) => {
                write!(f, "discovery root is not a directory: {}", path.display())
            }
            Self::Stale => f.write_str("discovery scan is obsolete"),
        }
    }
}

impl std::error::Error for DiscoveryFailure {}

#[derive(Clone, Debug, PartialEq)]
pub enum DiscoveryEvent {
    Batch(DiscoveryBatch),
    Complete(DiscoveryComplete),
    Error(DiscoveryError),
}

#[derive(Clone, Debug)]
pub struct DiscoveryRun {
    events: Vec<Result<DiscoveryEvent, DiscoveryFailure>>,
    cursor: usize,
}

impl DiscoveryRun {
    pub fn new(root: &Path, root_id: RootId, scan_id: ScanId) -> Result<Self, DiscoveryFailure> {
        let entries = discover_paths(root)?;
        let matched = entries.len() as u64;
        let events = batches(root_id, scan_id, entries)
            .into_iter()
            .map(|batch| Ok(DiscoveryEvent::Batch(batch)))
            .chain(std::iter::once(Ok(DiscoveryEvent::Complete(
                DiscoveryComplete {
                    root: root_id,
                    scan: scan_id,
                    matched,
                },
            ))))
            .collect();
        Ok(Self { events, cursor: 0 })
    }

    pub fn next_event(&mut self) -> Option<Result<DiscoveryEvent, DiscoveryFailure>> {
        let event = self.events.get(self.cursor).cloned();
        self.cursor += usize::from(event.is_some());
        event
    }
}

fn batches(root: RootId, scan: ScanId, paths: Vec<String>) -> Vec<DiscoveryBatch> {
    paths
        .chunks(MAX_BATCH_ITEMS)
        .map(|chunk| DiscoveryBatch {
            root,
            scan,
            entries: chunk
                .iter()
                .cloned()
                .map(|relative_path| DiscoveryEntry { relative_path })
                .collect(),
        })
        .collect()
}

#[derive(Debug)]
pub struct DiscoveryScanner {
    active_scan: Arc<AtomicU64>,
}

impl Default for DiscoveryScanner {
    fn default() -> Self {
        Self {
            active_scan: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl DiscoveryScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starting scan makes every earlier scan obsolete. Worker performs no UI work.
    pub fn spawn(
        &self,
        root: impl Into<PathBuf>,
        root_id: RootId,
        scan_id: ScanId,
    ) -> AsyncDiscovery {
        let root = root.into();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        self.active_scan.store(scan_id.get(), Ordering::Release);
        let active = Arc::clone(&self.active_scan);
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result =
                discover_paths_if_current(&root, root_id, scan_id, &active, &worker_cancelled);
            for batch in result.0 {
                if worker_cancelled.load(Ordering::Acquire)
                    || active.load(Ordering::Acquire) != scan_id.get()
                {
                    return;
                }
                if sender.send(Ok(DiscoveryEvent::Batch(batch))).is_err() {
                    return;
                }
            }
            if result.1 == 0 {
                let _ = sender.send(Ok(DiscoveryEvent::Complete(DiscoveryComplete {
                    root: root_id,
                    scan: scan_id,
                    matched: 0,
                })));
            } else if !worker_cancelled.load(Ordering::Acquire)
                && active.load(Ordering::Acquire) == scan_id.get()
            {
                let _ = sender.send(Ok(DiscoveryEvent::Complete(DiscoveryComplete {
                    root: root_id,
                    scan: scan_id,
                    matched: result.1,
                })));
            }
        });
        AsyncDiscovery {
            receiver,
            cancelled,
        }
    }

    pub fn cancel(&self, scan_id: ScanId) {
        if self.active_scan.load(Ordering::Acquire) == scan_id.get() {
            self.active_scan.store(0, Ordering::Release);
        }
    }

    pub fn is_current(&self, scan_id: ScanId) -> bool {
        self.active_scan.load(Ordering::Acquire) == scan_id.get()
    }
}

pub struct AsyncDiscovery {
    receiver: Receiver<Result<DiscoveryEvent, DiscoveryFailure>>,
    cancelled: Arc<AtomicBool>,
}

impl AsyncDiscovery {
    pub fn try_next(&self) -> Result<Option<DiscoveryEvent>, DiscoveryFailure> {
        match self.receiver.try_recv() {
            Ok(event) => event.map(Some),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Ok(None),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

fn discover_paths(root: &Path) -> Result<Vec<String>, DiscoveryFailure> {
    let metadata = fs::metadata(root).map_err(|error| map_discovery_error(root, error))?;
    if !metadata.is_dir() {
        return Err(DiscoveryFailure::NotADirectory(root.to_owned()));
    }
    let mut paths = Vec::new();
    walk(root, Path::new(""), &[], &mut |relative, _| {
        paths.push(relative.to_string_lossy().replace('\\', "/"));
        Ok(true)
    })?;
    paths.sort_unstable();
    Ok(paths)
}

fn discover_paths_if_current(
    root: &Path,
    root_id: RootId,
    scan_id: ScanId,
    active: &AtomicU64,
    cancelled: &AtomicBool,
) -> (Vec<DiscoveryBatch>, u64) {
    let mut pending = Vec::with_capacity(MAX_BATCH_ITEMS);
    let mut batches = Vec::new();
    let mut matched = 0;
    let result = walk(root, Path::new(""), &[], &mut |relative, _| {
        if cancelled.load(Ordering::Acquire) || active.load(Ordering::Acquire) != scan_id.get() {
            return Ok(false);
        }
        pending.push(relative.to_string_lossy().replace('\\', "/"));
        matched += 1;
        if pending.len() == MAX_BATCH_ITEMS {
            batches.push(DiscoveryBatch {
                root: root_id,
                scan: scan_id,
                entries: std::mem::take(&mut pending)
                    .into_iter()
                    .map(|relative_path| DiscoveryEntry { relative_path })
                    .collect(),
            });
        }
        Ok(true)
    });
    if result.is_err()
        || cancelled.load(Ordering::Acquire)
        || active.load(Ordering::Acquire) != scan_id.get()
    {
        return (Vec::new(), 0);
    }
    if !pending.is_empty() {
        batches.push(DiscoveryBatch {
            root: root_id,
            scan: scan_id,
            entries: pending
                .into_iter()
                .map(|relative_path| DiscoveryEntry { relative_path })
                .collect(),
        });
    }
    (batches, matched)
}

fn map_discovery_error(path: &Path, error: std::io::Error) -> DiscoveryFailure {
    match error.kind() {
        std::io::ErrorKind::NotFound => DiscoveryFailure::Missing(path.to_owned()),
        _ => DiscoveryFailure::Unreadable(path.to_owned()),
    }
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        })
}

fn walk<F>(
    root: &Path,
    relative_dir: &Path,
    inherited: &[IgnoreRule],
    on_file: &mut F,
) -> Result<(), DiscoveryFailure>
where
    F: FnMut(&Path, &Path) -> Result<bool, DiscoveryFailure>,
{
    let directory = root.join(relative_dir);
    let mut rules = inherited.to_vec();
    rules.extend(read_ignore_rules(&directory, relative_dir)?);
    let entries =
        fs::read_dir(&directory).map_err(|error| map_discovery_error(&directory, error))?;
    let mut children = entries
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| map_discovery_error(&directory, error))?;
    children.sort_by_key(|entry| entry.file_name());
    for entry in children {
        let name = entry.file_name();
        let child_relative = relative_dir.join(&name);
        if is_hidden(&child_relative) {
            continue;
        }
        let link_metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| map_discovery_error(&entry.path(), error))?;
        let is_link = link_metadata.file_type().is_symlink();
        let metadata = match fs::metadata(entry.path()) {
            Ok(metadata) => metadata,
            Err(_error) if is_link => continue,
            Err(error) => return Err(map_discovery_error(&entry.path(), error)),
        };
        let is_dir = metadata.is_dir();
        if ignored(&child_relative, is_dir, &rules) {
            continue;
        }
        if is_dir {
            if is_link {
                continue;
            }
            walk(root, &child_relative, &rules, on_file)?;
        } else if metadata.is_file()
            && is_markdown(&child_relative)
            && !on_file(&child_relative, &entry.path())?
        {
            return Ok(());
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct IgnoreRule {
    base: PathBuf,
    pattern: String,
    negated: bool,
    directory_only: bool,
    anchored: bool,
}

fn read_ignore_rules(
    directory: &Path,
    relative_dir: &Path,
) -> Result<Vec<IgnoreRule>, DiscoveryFailure> {
    let path = directory.join(".gitignore");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(DiscoveryFailure::Unreadable(path)),
    };
    Ok(text
        .lines()
        .filter_map(|line| parse_rule(line, relative_dir))
        .collect())
}

fn parse_rule(line: &str, base: &Path) -> Option<IgnoreRule> {
    let mut pattern = line.trim().to_owned();
    if pattern.is_empty() || pattern.starts_with('#') {
        return None;
    }
    let negated = pattern.starts_with('!') && !pattern.starts_with("\\!");
    if negated {
        pattern.remove(0);
    }
    let directory_only = pattern.ends_with('/');
    if directory_only {
        pattern.pop();
    }
    let anchored = pattern.starts_with('/');
    if anchored {
        pattern.remove(0);
    }
    if pattern.is_empty() {
        return None;
    }
    Some(IgnoreRule {
        base: base.to_owned(),
        pattern,
        negated,
        directory_only,
        anchored,
    })
}

fn ignored(path: &Path, is_dir: bool, rules: &[IgnoreRule]) -> bool {
    let mut ignored = false;
    for rule in rules {
        let Ok(relative) = path.strip_prefix(&rule.base) else {
            continue;
        };
        let candidate = relative.to_string_lossy().replace('\\', "/");
        let matches = if rule.pattern.contains('/') {
            if rule.anchored {
                glob_matches(&rule.pattern, &candidate)
            } else {
                candidate.split_once('/').map_or_else(
                    || glob_matches(&rule.pattern, &candidate),
                    |_| {
                        candidate.split('/').enumerate().any(|(index, _)| {
                            glob_matches(
                                &rule.pattern,
                                &candidate
                                    .split('/')
                                    .skip(index)
                                    .collect::<Vec<_>>()
                                    .join("/"),
                            )
                        })
                    },
                )
            }
        } else {
            candidate
                .split('/')
                .any(|part| glob_matches(&rule.pattern, part))
        };
        if matches && (!rule.directory_only || is_dir) {
            ignored = !rule.negated;
        }
    }
    ignored
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    fn go(pattern: &[char], value: &[char]) -> bool {
        match pattern.first() {
            None => value.is_empty(),
            Some('*') => {
                if pattern.get(1) == Some(&'*') {
                    go(&pattern[1..], value) || (!value.is_empty() && go(pattern, &value[1..]))
                } else {
                    go(&pattern[1..], value)
                        || (!value.is_empty() && value[0] != '/' && go(pattern, &value[1..]))
                }
            }
            Some('?') => !value.is_empty() && value[0] != '/' && go(&pattern[1..], &value[1..]),
            Some(character) => value.first() == Some(character) && go(&pattern[1..], &value[1..]),
        }
    }
    go(
        &pattern.chars().collect::<Vec<_>>(),
        &value.chars().collect::<Vec<_>>(),
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileStamp {
    length: u64,
    modified: Option<SystemTime>,
    is_file: bool,
}

fn stamp(path: &Path) -> Result<Option<FileStamp>, DiscoveryFailure> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(FileStamp {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            is_file: metadata.is_file(),
        })),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(DiscoveryFailure::Unreadable(path.to_owned())),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PollEvent {
    Changed,
    Deleted,
    Reappeared,
    ParentChanged,
    Unchanged,
}

#[derive(Debug)]
pub struct PollingWatcher {
    path: PathBuf,
    parent: PathBuf,
    file: Option<FileStamp>,
    parent_stamp: Option<FileStamp>,
}

impl PollingWatcher {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, DiscoveryFailure> {
        let path = path.into();
        let parent = path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_owned);
        Ok(Self {
            file: stamp(&path)?,
            parent_stamp: stamp(&parent)?,
            path,
            parent,
        })
    }

    pub fn poll(&mut self) -> Result<PollEvent, DiscoveryFailure> {
        let file = stamp(&self.path)?;
        let parent = stamp(&self.parent)?;
        let event = match (&self.file, &file) {
            (Some(_), None) => PollEvent::Deleted,
            (None, Some(_)) => PollEvent::Reappeared,
            (Some(old), Some(new)) if old != new => PollEvent::Changed,
            _ if self.parent_stamp != parent => PollEvent::ParentChanged,
            _ => PollEvent::Unchanged,
        };
        self.file = file;
        self.parent_stamp = parent;
        Ok(event)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReloadRequest {
    pub generation: Generation,
}

#[derive(Debug)]
pub struct ReloadState {
    path: PathBuf,
    next_generation: u64,
    active: Generation,
    visible: Option<LoadedSource>,
    error: Option<LoadError>,
    pending_since: Option<SystemTime>,
}

impl ReloadState {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let generation = Generation::new(1).expect("nonzero generation");
        Self {
            path,
            next_generation: 1,
            active: generation,
            visible: None,
            error: None,
            pending_since: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn visible(&self) -> Option<&LoadedSource> {
        self.visible.as_ref()
    }

    pub fn error(&self) -> Option<&LoadError> {
        self.error.as_ref()
    }

    pub fn request(&mut self, now: SystemTime) -> ReloadRequest {
        self.next_generation = self.next_generation.saturating_add(1);
        self.active = Generation::new(self.next_generation).expect("generation counter overflow");
        self.pending_since = Some(now);
        ReloadRequest {
            generation: self.active,
        }
    }

    pub fn ready(&mut self, request: ReloadRequest, source: LoadedSource) -> bool {
        if request.generation != self.active {
            return false;
        }
        self.visible = Some(source);
        self.error = None;
        self.pending_since = None;
        true
    }

    pub fn failed(&mut self, request: ReloadRequest, error: LoadError) -> bool {
        if request.generation != self.active {
            return false;
        }
        self.error = Some(error);
        self.pending_since = None;
        true
    }

    pub fn debounce_elapsed(&self, now: SystemTime) -> bool {
        self.pending_since
            .is_some_and(|since| now.duration_since(since).unwrap_or_default() >= RELOAD_DEBOUNCE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
        time::UNIX_EPOCH,
    };

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let suffix = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("mdvr-files-{}-{suffix}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn resolves_relative_paths_and_empty_source() {
        let root = temp_dir();
        let file = root.join("empty.MD");
        fs::write(&file, "").unwrap();
        let resolved = resolve_path(&root, Some(Path::new("empty.MD"))).unwrap();
        assert_eq!(resolved.path, file);
        assert!(load_source(&file, false).unwrap().is_empty());
        assert!(matches!(
            resolve_path(&root, Some(Path::new("missing.md"))),
            Err(PathError::Missing(_))
        ));
    }

    #[test]
    fn discovery_honors_ignores_hidden_files_and_symlink_directories() {
        let root = temp_dir();
        fs::create_dir_all(root.join("nested/ignored")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join(".gitignore"), "nested/ignored/\nignored.md\n").unwrap();
        fs::write(root.join("README.MARKDOWN"), "ok").unwrap();
        fs::write(root.join("ignored.md"), "no").unwrap();
        fs::write(root.join("nested/keep.md"), "ok").unwrap();
        fs::write(root.join("nested/ignored/no.md"), "no").unwrap();
        fs::write(root.join(".hidden/no.md"), "no").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("nested"), root.join("linked")).unwrap();
        let paths = discover_paths(&root).unwrap();
        assert_eq!(paths, vec!["README.MARKDOWN", "nested/keep.md"]);
    }

    #[test]
    fn batches_are_bounded_and_stale_async_scan_drops_events() {
        let root = temp_dir();
        for index in 0..(MAX_BATCH_ITEMS + 1) {
            fs::write(root.join(format!("{index}.md")), "x").unwrap();
        }
        let scanner = DiscoveryScanner::new();
        let first = scanner.spawn(&root, RootId::new(1).unwrap(), ScanId::new(1).unwrap());
        let second = scanner.spawn(&root, RootId::new(2).unwrap(), ScanId::new(2).unwrap());
        first.cancel();
        assert!(scanner.is_current(ScanId::new(2).unwrap()));
        let mut run =
            DiscoveryRun::new(&root, RootId::new(2).unwrap(), ScanId::new(2).unwrap()).unwrap();
        let mut count = 0;
        while let Some(Ok(DiscoveryEvent::Batch(batch))) = run.next_event() {
            assert!(batch.entries.len() <= MAX_BATCH_ITEMS);
            count += batch.entries.len();
        }
        assert_eq!(count, MAX_BATCH_ITEMS + 1);
        drop(second);
    }

    #[test]
    fn reload_rejects_stale_and_preserves_last_good_on_error() {
        let root = temp_dir();
        let path = root.join("doc.md");
        fs::write(&path, "before").unwrap();
        let source = load_source(&path, false).unwrap();
        let mut state = ReloadState::new(&path);
        let first = state.request(UNIX_EPOCH);
        assert!(state.ready(first, source.clone()));
        let old = state.request(UNIX_EPOCH);
        let current = state.request(UNIX_EPOCH);
        assert!(!state.failed(old, LoadError::Missing(path.clone())));
        assert!(state.failed(current, LoadError::Missing(path)));
        assert_eq!(state.visible(), Some(&source));
    }
}
