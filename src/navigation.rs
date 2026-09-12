//! Transactional local navigation, history, anchors, and reload generations.
//!
//! File reads are supplied by the caller. A failed or stale completion never
//! changes current document or history.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use crate::{
    contracts::{DocumentId, Generation, NavigationTarget, RequestId},
    files::LoadedSource,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Locator {
    pub heading: Option<String>,
    pub block: String,
    pub offset: u32,
}

impl Locator {
    pub fn start() -> Self {
        Self {
            heading: None,
            block: "document-start".into(),
            offset: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentState {
    pub document: DocumentId,
    pub generation: Generation,
    pub path: PathBuf,
    pub source: String,
    pub anchor: Option<String>,
    pub locator: Locator,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry {
    pub path: PathBuf,
    pub anchor: Option<String>,
    pub locator: Locator,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NavigationAction {
    Anchor { request: RequestId, anchor: String },
    Load(LoadRequest),
    External(NavigationTarget),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadRequest {
    pub request: RequestId,
    pub source_document: DocumentId,
    pub source_generation: Generation,
    pub generation: Generation,
    pub path: PathBuf,
    pub anchor: Option<String>,
    kind: LoadKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoadKind {
    New,
    Back { cursor: usize },
    Forward { cursor: usize },
    Reload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NavigationFailure {
    NoCurrentDocument,
    StaleSource,
    UnsupportedLocalLink(PathBuf),
    InvalidTarget(String),
    LoadFailed(PathBuf),
    StaleCompletion,
}

impl fmt::Display for NavigationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCurrentDocument => f.write_str("no current document"),
            Self::StaleSource => f.write_str("navigation source is stale"),
            Self::UnsupportedLocalLink(path) => {
                write!(f, "local link is not Markdown: {}", path.display())
            }
            Self::InvalidTarget(target) => write!(f, "invalid navigation target: {target}"),
            Self::LoadFailed(path) => write!(f, "navigation load failed: {}", path.display()),
            Self::StaleCompletion => f.write_str("navigation completion is stale"),
        }
    }
}

impl std::error::Error for NavigationFailure {}

#[derive(Debug)]
pub struct NavigationState {
    root: PathBuf,
    current: Option<DocumentState>,
    history: Vec<HistoryEntry>,
    cursor: Option<usize>,
    next_id: u64,
    next_generation: u64,
    pending: Option<LoadRequest>,
}

impl NavigationState {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            current: None,
            history: Vec::new(),
            cursor: None,
            next_id: 0,
            next_generation: 0,
            pending: None,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn current(&self) -> Option<&DocumentState> {
        self.current.as_ref()
    }

    pub fn history(&self) -> &[HistoryEntry] {
        &self.history
    }

    pub fn history_cursor(&self) -> Option<usize> {
        self.cursor
    }

    pub fn can_go_back(&self) -> bool {
        self.cursor.is_some_and(|cursor| cursor > 0)
    }

    pub fn can_go_forward(&self) -> bool {
        self.cursor
            .is_some_and(|cursor| cursor + 1 < self.history.len())
    }

    /// Initial/explicit open commits only after caller has read source.
    pub fn open_initial(&mut self, source: LoadedSource) -> DocumentId {
        let document = self.new_document_id();
        let generation = self.new_generation();
        self.current = Some(DocumentState {
            document,
            generation,
            path: source.path.clone(),
            source: source.source,
            anchor: None,
            locator: Locator::start(),
        });
        self.history = vec![HistoryEntry {
            path: source.path,
            anchor: None,
            locator: Locator::start(),
        }];
        self.cursor = Some(0);
        self.pending = None;
        document
    }

    /// Resolve a renderer link. Markdown loads remain pending until commit.
    pub fn request_navigation(
        &mut self,
        target: &str,
        locator: Locator,
    ) -> Result<NavigationAction, NavigationFailure> {
        let current = self
            .current
            .as_ref()
            .ok_or(NavigationFailure::NoCurrentDocument)?;
        self.request_navigation_from(target, current.generation, locator)
    }

    pub fn request_navigation_from(
        &mut self,
        target: &str,
        source_generation: Generation,
        locator: Locator,
    ) -> Result<NavigationAction, NavigationFailure> {
        let current = self
            .current
            .as_ref()
            .ok_or(NavigationFailure::NoCurrentDocument)?;
        if current.generation != source_generation {
            return Err(NavigationFailure::StaleSource);
        }
        let (path, anchor) = split_target(target)?;
        if path.as_os_str().is_empty() {
            let anchor = anchor.ok_or_else(|| NavigationFailure::InvalidTarget(target.into()))?;
            let request = self.new_request_id();
            self.push_anchor(anchor.clone(), locator);
            return Ok(NavigationAction::Anchor { request, anchor });
        }
        if is_remote(&path) {
            return Ok(NavigationAction::External(remote_target(target, &path)));
        }
        let resolved = if path.is_absolute() {
            path
        } else {
            current.path.parent().unwrap_or(Path::new(".")).join(path)
        };
        if is_markdown(&resolved) {
            let request = self.make_load_request(resolved, anchor, locator, LoadKind::New);
            Ok(NavigationAction::Load(request))
        } else {
            Ok(NavigationAction::External(NavigationTarget::LocalFile {
                path: resolved.to_string_lossy().into_owned(),
            }))
        }
    }

    pub fn go_back(&mut self, locator: Locator) -> Result<Option<LoadRequest>, NavigationFailure> {
        self.request_history_move(locator, -1)
    }

    pub fn go_forward(
        &mut self,
        locator: Locator,
    ) -> Result<Option<LoadRequest>, NavigationFailure> {
        self.request_history_move(locator, 1)
    }

    fn request_history_move(
        &mut self,
        locator: Locator,
        direction: isize,
    ) -> Result<Option<LoadRequest>, NavigationFailure> {
        self.current
            .as_ref()
            .ok_or(NavigationFailure::NoCurrentDocument)?;
        let Some(cursor) = self.cursor else {
            return Ok(None);
        };
        let target = cursor as isize + direction;
        if target < 0 || target as usize >= self.history.len() {
            return Ok(None);
        }
        if let Some(entry) = self.history.get_mut(cursor) {
            entry.locator = locator;
        }
        let target = target as usize;
        let entry = self.history[target].clone();
        let kind = if direction < 0 {
            LoadKind::Back { cursor: target }
        } else {
            LoadKind::Forward { cursor: target }
        };
        Ok(Some(self.make_load_request(
            entry.path,
            entry.anchor,
            Locator::start(),
            kind,
        )))
    }

    pub fn reload(&mut self) -> Result<LoadRequest, NavigationFailure> {
        let (path, anchor, locator) = {
            let current = self
                .current
                .as_ref()
                .ok_or(NavigationFailure::NoCurrentDocument)?;
            (
                current.path.clone(),
                current.anchor.clone(),
                current.locator.clone(),
            )
        };
        let generation = self.new_generation();
        Ok(self.make_load_request_at(path, anchor, locator, LoadKind::Reload, generation))
    }

    /// Match reload worker generations so stale file results cannot replace newer views.
    pub fn reload_at(&mut self, generation: Generation) -> Result<LoadRequest, NavigationFailure> {
        let current = self
            .current
            .as_ref()
            .ok_or(NavigationFailure::NoCurrentDocument)?;
        if generation.get() <= current.generation.get() || generation.get() <= self.next_generation
        {
            return Err(NavigationFailure::StaleCompletion);
        }
        self.next_generation = generation.get();
        Ok(self.make_load_request_at(
            current.path.clone(),
            current.anchor.clone(),
            current.locator.clone(),
            LoadKind::Reload,
            generation,
        ))
    }

    /// Commit successful source read. Stale requests and failed reads leave state untouched.
    pub fn commit_load(
        &mut self,
        request: LoadRequest,
        source: LoadedSource,
    ) -> Result<DocumentId, NavigationFailure> {
        if self.pending.as_ref() != Some(&request)
            || request.generation
                != self
                    .pending
                    .as_ref()
                    .map_or(request.generation, |pending| pending.generation)
        {
            return Err(NavigationFailure::StaleCompletion);
        }
        if source.path != request.path {
            return Err(NavigationFailure::LoadFailed(source.path));
        }
        let document = match request.kind {
            LoadKind::Reload => match self.current.as_ref() {
                Some(current) => current.document,
                None => self.new_document_id(),
            },
            _ => self.new_document_id(),
        };
        match request.kind {
            LoadKind::New => {
                let cursor = self.cursor.unwrap_or(0);
                self.history.truncate(cursor + 1);
                self.history.push(HistoryEntry {
                    path: request.path.clone(),
                    anchor: request.anchor.clone(),
                    locator: Locator::start(),
                });
                self.cursor = Some(self.history.len() - 1);
            }
            LoadKind::Back { cursor } | LoadKind::Forward { cursor } => {
                self.cursor = Some(cursor);
            }
            LoadKind::Reload => {
                if let Some(cursor) = self.cursor {
                    self.history[cursor].path = request.path.clone();
                    self.history[cursor].anchor = request.anchor.clone();
                }
            }
        }
        let locator = match request.kind {
            LoadKind::Back { .. } | LoadKind::Forward { .. } => {
                self.history[self.cursor.unwrap()].locator.clone()
            }
            _ => request_locator(&request),
        };
        self.current = Some(DocumentState {
            document,
            generation: request.generation,
            path: source.path,
            source: source.source,
            anchor: request.anchor,
            locator,
        });
        self.pending = None;
        Ok(document)
    }

    pub fn fail_load(&mut self, request: &LoadRequest) -> bool {
        if self.pending.as_ref() == Some(request) {
            self.pending = None;
            true
        } else {
            false
        }
    }

    fn push_anchor(&mut self, anchor: String, locator: Locator) {
        let Some(current) = self.current.as_mut() else {
            return;
        };
        if let Some(cursor) = self.cursor {
            self.history[cursor].locator = locator.clone();
            self.history.truncate(cursor + 1);
            self.history.push(HistoryEntry {
                path: current.path.clone(),
                anchor: Some(anchor.clone()),
                locator: locator.clone(),
            });
            self.cursor = Some(self.history.len() - 1);
        }
        current.anchor = Some(anchor);
        current.locator = locator;
    }

    fn make_load_request(
        &mut self,
        path: PathBuf,
        anchor: Option<String>,
        locator: Locator,
        kind: LoadKind,
    ) -> LoadRequest {
        let generation = self.new_generation();
        self.make_load_request_at(path, anchor, locator, kind, generation)
    }

    fn make_load_request_at(
        &mut self,
        path: PathBuf,
        anchor: Option<String>,
        locator: Locator,
        kind: LoadKind,
        generation: Generation,
    ) -> LoadRequest {
        let (source_document, source_generation) = {
            let current = self
                .current
                .as_ref()
                .expect("navigation requires current document");
            (current.document, current.generation)
        };
        let request = LoadRequest {
            request: self.new_request_id(),
            source_document,
            source_generation,
            generation,
            path,
            anchor,
            kind,
        };
        self.pending = Some(request.clone());
        if matches!(kind, LoadKind::New) {
            // Locator captured before transition; history is changed only by successful commit.
            if let Some(cursor) = self.cursor {
                self.history[cursor].locator = locator;
            }
        }
        request
    }

    fn new_document_id(&mut self) -> DocumentId {
        self.next_id = self.next_id.saturating_add(1);
        DocumentId::new(self.next_id).expect("document id overflow")
    }

    fn new_request_id(&mut self) -> RequestId {
        self.next_id = self.next_id.saturating_add(1);
        RequestId::new(self.next_id).expect("request id overflow")
    }

    fn new_generation(&mut self) -> Generation {
        self.next_generation = self.next_generation.saturating_add(1);
        Generation::new(self.next_generation).expect("generation overflow")
    }
}

fn request_locator(request: &LoadRequest) -> Locator {
    Locator {
        heading: request.anchor.clone(),
        block: "document-start".into(),
        offset: 0,
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        })
}

fn is_remote(path: &Path) -> bool {
    path.to_string_lossy().starts_with("http://")
        || path.to_string_lossy().starts_with("https://")
        || path.to_string_lossy().starts_with("mailto:")
}

fn remote_target(target: &str, path: &Path) -> NavigationTarget {
    if path.to_string_lossy().starts_with("mailto:") {
        NavigationTarget::Mailto { url: target.into() }
    } else {
        NavigationTarget::Http { url: target.into() }
    }
}

fn split_target(target: &str) -> Result<(PathBuf, Option<String>), NavigationFailure> {
    if target.is_empty() {
        return Err(NavigationFailure::InvalidTarget(target.into()));
    }
    let (path, anchor) = target
        .split_once('#')
        .map_or((target, None), |(path, anchor)| {
            (path, Some(anchor.to_owned()))
        });
    if path.starts_with('#') || path.is_empty() {
        return Ok((PathBuf::new(), anchor));
    }
    if path.contains('\0') {
        return Err(NavigationFailure::InvalidTarget(target.into()));
    }
    Ok((PathBuf::from(percent_decode(path)?), anchor))
}

fn percent_decode(value: &str) -> Result<String, NavigationFailure> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(NavigationFailure::InvalidTarget(value.into()));
            }
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);
            match (high, low) {
                (Some(high), Some(low)) => output.push((high * 16 + low) as u8),
                _ => return Err(NavigationFailure::InvalidTarget(value.into())),
            }
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|_| NavigationFailure::InvalidTarget(value.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::load_source;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn relative_markdown_navigation_and_anchor_commit_only_on_success() {
        let source = load_source(&fixture("reload/before.md"), false).unwrap();
        let mut state = NavigationState::new(fixture(""));
        state.open_initial(source);
        let action = state
            .request_navigation("../links/target.md#café--日本語", Locator::start())
            .unwrap();
        let request = match action {
            NavigationAction::Load(request) => request,
            other => panic!("unexpected action: {other:?}"),
        };
        assert_eq!(request.anchor.as_deref(), Some("café--日本語"));
        assert!(state.fail_load(&request));
        assert_eq!(state.history().len(), 1);
        assert_eq!(state.current().unwrap().path, fixture("reload/before.md"));
    }

    #[test]
    fn history_branch_truncates_after_new_successful_load() {
        let first = load_source(&fixture("reload/before.md"), false).unwrap();
        let second = load_source(&fixture("reload/after.md"), false).unwrap();
        let mut state = NavigationState::new(fixture(""));
        state.open_initial(first.clone());
        let action = state
            .request_navigation(
                "after.md",
                Locator {
                    heading: Some("stable-heading".into()),
                    block: "p".into(),
                    offset: 2,
                },
            )
            .unwrap();
        let request = match action {
            NavigationAction::Load(request) => request,
            _ => unreachable!(),
        };
        state.commit_load(request, second.clone()).unwrap();
        let back = state.go_back(Locator::start()).unwrap().unwrap();
        state.commit_load(back, first).unwrap();
        let action = state
            .request_navigation("after.md", Locator::start())
            .unwrap();
        let request = match action {
            NavigationAction::Load(request) => request,
            _ => unreachable!(),
        };
        state.commit_load(request, second).unwrap();
        assert!(!state.can_go_forward());
        assert_eq!(state.history().len(), 2);
    }

    #[test]
    fn reload_and_stale_completion_preserve_last_good_document() {
        let path = fixture("reload/before.md");
        let source = load_source(&path, false).unwrap();
        let mut state = NavigationState::new(fixture(""));
        state.open_initial(source.clone());
        let first = state.reload().unwrap();
        let second = state.reload().unwrap();
        assert!(matches!(
            state.commit_load(first, source.clone()),
            Err(NavigationFailure::StaleCompletion)
        ));
        assert!(state.commit_load(second, source).is_ok());
    }

    #[test]
    fn remote_and_non_markdown_targets_leave_native_policy_in_charge() {
        let source = load_source(&fixture("reload/before.md"), false).unwrap();
        let mut state = NavigationState::new(fixture(""));
        state.open_initial(source);
        assert!(matches!(
            state
                .request_navigation("https://example.test", Locator::start())
                .unwrap(),
            NavigationAction::External(NavigationTarget::Http { .. })
        ));
        assert!(matches!(
            state
                .request_navigation("../documents/assets/one.png", Locator::start())
                .unwrap(),
            NavigationAction::External(NavigationTarget::LocalFile { .. })
        ));
    }

    #[test]
    fn atomic_replacement_is_seen_by_polling_core() {
        let suffix = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("mdvr-navigation-{}-{suffix}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("doc.md");
        fs::write(&path, "before").unwrap();
        let mut watcher = crate::files::PollingWatcher::new(&path).unwrap();
        let replacement = root.join("replacement.md");
        fs::write(&replacement, "after").unwrap();
        fs::rename(replacement, &path).unwrap();
        assert!(matches!(
            watcher.poll().unwrap(),
            crate::files::PollEvent::Changed
        ));
    }
}
