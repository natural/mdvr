//! GPUI shell state for contract revision 1.
//!
//! This module owns shell decisions only. Document parsing and rendering stay
//! behind the contract boundary.

use std::{cmp::Ordering, collections::BTreeSet, path::PathBuf};

use crate::contracts::{
    ContractError, DiscoveryBatch, DiscoveryComplete, DiscoveryError, ErrorCode, RootId, ScanId,
};

pub const CONTRACT_REVISION: u16 = crate::contracts::CONTRACT_REVISION;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PickerEntry {
    pub relative_path: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PickerStatus {
    Scanning,
    Complete,
    Error { code: ErrorCode, message: String },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PickerState {
    query: String,
    entries: BTreeSet<String>,
    selected: Option<String>,
    status: Option<PickerStatus>,
}

impl PickerState {
    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.retain_or_select(0);
    }

    pub fn status(&self) -> Option<&PickerStatus> {
        self.status.as_ref()
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn visible(&self) -> Vec<PickerEntry> {
        let mut matches = self
            .entries
            .iter()
            .filter_map(|path| fuzzy_score(path, &self.query).map(|score| (path, score)))
            .collect::<Vec<_>>();
        matches.sort_by(|(left_path, left_score), (right_path, right_score)| {
            if self.query.is_empty() {
                path_order(left_path, right_path)
            } else {
                right_score
                    .cmp(left_score)
                    .then_with(|| path_order(left_path, right_path))
            }
        });
        matches
            .into_iter()
            .map(|(path, _)| PickerEntry {
                relative_path: path.clone(),
            })
            .collect()
    }

    pub fn replace_entries<I, S>(&mut self, entries: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let selected = self.selected.clone();
        self.entries = entries
            .into_iter()
            .map(Into::into)
            .filter(|path| is_markdown_file(path))
            .collect();
        self.selected = selected;
        self.retain_or_select(0);
    }

    pub fn apply_batch(
        &mut self,
        batch: &DiscoveryBatch,
        root: RootId,
        scan: ScanId,
    ) -> Result<(), ContractError> {
        batch.validate_for(root, scan)?;
        self.status = Some(PickerStatus::Scanning);
        let selected = self.selected.clone();
        self.entries.extend(
            batch
                .entries
                .iter()
                .map(|entry| entry.relative_path.clone())
                .filter(|path| is_markdown_file(path)),
        );
        self.selected = selected;
        self.retain_or_select(0);
        Ok(())
    }

    pub fn complete(
        &mut self,
        message: &DiscoveryComplete,
        root: RootId,
        scan: ScanId,
    ) -> Result<(), ContractError> {
        message.validate_for(root, scan)?;
        self.status = Some(PickerStatus::Complete);
        self.retain_or_select(0);
        Ok(())
    }

    pub fn fail(
        &mut self,
        message: &DiscoveryError,
        root: RootId,
        scan: ScanId,
    ) -> Result<(), ContractError> {
        message.validate_for(root, scan)?;
        self.status = Some(PickerStatus::Error {
            code: message.code.clone(),
            message: message.message.clone(),
        });
        Ok(())
    }

    pub fn move_selection(&mut self, delta: isize) {
        let visible = self.visible();
        if visible.is_empty() {
            self.selected = None;
            return;
        }
        let current = self
            .selected
            .as_deref()
            .and_then(|selected| {
                visible
                    .iter()
                    .position(|entry| entry.relative_path == selected)
            })
            .unwrap_or(0) as isize;
        let next = (current + delta).rem_euclid(visible.len() as isize) as usize;
        self.selected = Some(visible[next].relative_path.clone());
    }

    pub fn activate(&self) -> PickerAction {
        self.selected
            .as_ref()
            .filter(|path| {
                self.visible()
                    .iter()
                    .any(|entry| &entry.relative_path == *path)
            })
            .cloned()
            .map_or(PickerAction::ChooseFolder, PickerAction::Open)
    }

    pub fn escape(&self, current_document: Option<PathBuf>) -> PickerAction {
        current_document.map_or(PickerAction::ChooseFolder, PickerAction::ReturnToDocument)
    }

    fn retain_or_select(&mut self, preferred_index: usize) {
        let visible = self.visible();
        if self
            .selected
            .as_ref()
            .is_some_and(|selected| visible.iter().any(|entry| &entry.relative_path == selected))
        {
            return;
        }
        self.selected = visible
            .get(preferred_index.min(visible.len().saturating_sub(1)))
            .map(|entry| entry.relative_path.clone());
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PickerAction {
    Open(String),
    ReturnToDocument(PathBuf),
    ChooseFolder,
}

fn is_markdown_file(path: &str) -> bool {
    !path.is_empty()
        && !path.ends_with('/')
        && PathBuf::from(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown")
            })
}

fn path_order(left: &str, right: &str) -> Ordering {
    left.to_ascii_lowercase()
        .cmp(&right.to_ascii_lowercase())
        .then_with(|| left.cmp(right))
}

fn fuzzy_score(path: &str, query: &str) -> Option<(i32, i32, i32)> {
    if query.is_empty() {
        return Some((0, 0, 0));
    }
    let path = path.to_ascii_lowercase();
    let query = query.to_ascii_lowercase();
    let mut cursor = 0;
    let mut first = None;
    let mut previous = None;
    let mut score = 0;
    for character in query.chars() {
        let position = path[cursor..].find(character)? + cursor;
        first.get_or_insert(position);
        score += if previous == Some(position.saturating_sub(1)) {
            12
        } else {
            0
        };
        score += if position == 0
            || path.as_bytes().get(position - 1).is_some_and(|byte| {
                *byte == b'/' || *byte == b'_' || *byte == b'-' || *byte == b' '
            }) {
            8
        } else {
            0
        };
        previous = Some(position);
        cursor = position + character.len_utf8();
    }
    let first = first.unwrap_or(0);
    Some((
        score - first as i32,
        -(path.len() as i32),
        -(query.len() as i32),
    ))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusOwner {
    Shell,
    Picker,
    Search,
    Palette,
    Renderer,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusState {
    owner: FocusOwner,
    restore: Vec<FocusOwner>,
}

impl Default for FocusState {
    fn default() -> Self {
        Self {
            owner: FocusOwner::Renderer,
            restore: Vec::new(),
        }
    }
}

impl FocusState {
    pub fn owner(&self) -> FocusOwner {
        self.owner
    }

    pub fn open(&mut self, owner: FocusOwner) {
        if self.owner != owner {
            self.restore.push(self.owner);
            self.owner = owner;
        }
    }

    pub fn close(&mut self, owner: FocusOwner) {
        if self.owner == owner {
            self.owner = self.restore.pop().unwrap_or(FocusOwner::Shell);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry {
    pub path: PathBuf,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct History {
    back: Vec<HistoryEntry>,
    forward: Vec<HistoryEntry>,
}

impl History {
    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn push(&mut self, path: PathBuf) {
        if self.back.last().is_some_and(|entry| entry.path == path) {
            return;
        }
        self.back.push(HistoryEntry { path });
        self.forward.clear();
    }

    pub fn back(&mut self) -> Option<PathBuf> {
        let entry = self.back.pop()?;
        self.forward.push(entry);
        self.back.last().map(|entry| entry.path.clone())
    }

    pub fn forward(&mut self) -> Option<PathBuf> {
        let entry = self.forward.pop()?;
        let path = entry.path.clone();
        self.back.push(entry);
        Some(path)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupError {
    pub failed_path: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoveryAction {
    Retry(PathBuf),
    ChooseFile,
    BrowseFolder,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaletteAction {
    SwitchThemeFamily(String),
    ToggleOutline,
    NavigateHeading(String),
    CopyMarkdown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellCommand {
    ChooseFile,
    ChooseFolder,
    OpenPicker,
    OpenSearch,
    GoBack,
    GoForward,
    Reload,
    CloseWindow,
    OpenPalette,
    IncreaseTextSize,
    DecreaseTextSize,
    ResetTextSize,
    SwitchThemeFamily(String),
    ToggleOutline,
    NavigateHeading(String),
    CopyMarkdown,
    RetryStartup(PathBuf),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Shortcut {
    pub command: bool,
    pub shift: bool,
    pub key: char,
}

impl Shortcut {
    pub const fn command(key: char) -> Self {
        Self {
            command: true,
            shift: false,
            key,
        }
    }

    pub const fn command_shift(key: char) -> Self {
        Self {
            command: true,
            shift: true,
            key,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShellState {
    pub picker: PickerState,
    pub focus: FocusState,
    pub history: History,
    pub root: Option<PathBuf>,
    pub current_document: Option<PathBuf>,
    pub startup_error: Option<StartupError>,
    pub outline_visible: bool,
    pub text_scale_percent: u16,
}

impl ShellState {
    pub fn new() -> Self {
        Self {
            text_scale_percent: 100,
            ..Self::default()
        }
    }

    pub fn dispatch_shortcut(&mut self, shortcut: Shortcut) -> Option<ShellCommand> {
        let command = match (shortcut.command, shortcut.shift, shortcut.key) {
            (true, false, 'o') => ShellCommand::ChooseFile,
            (true, true, 'o') => ShellCommand::ChooseFolder,
            (true, false, 'p') => ShellCommand::OpenPicker,
            (true, false, 'f') => ShellCommand::OpenSearch,
            (true, false, '[') if self.history.can_go_back() => ShellCommand::GoBack,
            (true, false, ']') if self.history.can_go_forward() => ShellCommand::GoForward,
            (true, false, 'r') => ShellCommand::Reload,
            (true, false, 'w') => ShellCommand::CloseWindow,
            (true, true, 'p') => ShellCommand::OpenPalette,
            (true, false, '+') => ShellCommand::IncreaseTextSize,
            (true, false, '-') => ShellCommand::DecreaseTextSize,
            (true, false, '0') => ShellCommand::ResetTextSize,
            _ => return None,
        };
        self.dispatch(command.clone());
        Some(command)
    }

    pub fn dispatch(&mut self, command: ShellCommand) {
        match command {
            ShellCommand::OpenPicker => self.focus.open(FocusOwner::Picker),
            ShellCommand::OpenSearch => self.focus.open(FocusOwner::Search),
            ShellCommand::OpenPalette => self.focus.open(FocusOwner::Palette),
            ShellCommand::GoBack => {
                if let Some(path) = self.history.back() {
                    self.current_document = Some(path);
                }
            }
            ShellCommand::GoForward => {
                if let Some(path) = self.history.forward() {
                    self.current_document = Some(path);
                }
            }
            ShellCommand::IncreaseTextSize => {
                self.text_scale_percent = (self.text_scale_percent + 10).min(300)
            }
            ShellCommand::DecreaseTextSize => {
                self.text_scale_percent = self.text_scale_percent.saturating_sub(10).max(50)
            }
            ShellCommand::ResetTextSize => self.text_scale_percent = 100,
            ShellCommand::ToggleOutline => self.outline_visible = !self.outline_visible,
            ShellCommand::SwitchThemeFamily(_)
            | ShellCommand::NavigateHeading(_)
            | ShellCommand::CopyMarkdown => {}
            _ => {}
        }
    }

    pub fn dispatch_palette(&mut self, action: PaletteAction) -> ShellCommand {
        let command = match action {
            PaletteAction::SwitchThemeFamily(family) => ShellCommand::SwitchThemeFamily(family),
            PaletteAction::ToggleOutline => ShellCommand::ToggleOutline,
            PaletteAction::NavigateHeading(heading) => ShellCommand::NavigateHeading(heading),
            PaletteAction::CopyMarkdown => ShellCommand::CopyMarkdown,
        };
        self.dispatch(command.clone());
        command
    }

    pub fn close_focus(&mut self, owner: FocusOwner) {
        self.focus.close(owner);
    }

    pub fn startup_failed(&mut self, path: PathBuf, error: impl Into<String>) {
        self.startup_error = Some(StartupError {
            failed_path: path,
            message: error.into(),
        });
    }

    pub fn recover(&mut self, action: RecoveryAction) -> ShellCommand {
        let command = match action {
            RecoveryAction::Retry(path) => ShellCommand::RetryStartup(path),
            RecoveryAction::ChooseFile => ShellCommand::ChooseFile,
            RecoveryAction::BrowseFolder => ShellCommand::ChooseFolder,
        };
        self.startup_error = None;
        command
    }
}

#[cfg(target_os = "macos")]
pub struct ShellView {
    pub state: ShellState,
}

#[cfg(target_os = "macos")]
impl ShellView {
    pub fn new() -> Self {
        Self {
            state: ShellState::new(),
        }
    }
}

#[cfg(target_os = "macos")]
impl gpui::Render for ShellView {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::{div, prelude::*};
        div().size_full().child("mdvr shell")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::DiscoveryEntry;

    fn id<T>(value: u64) -> T
    where
        T: Id,
    {
        T::new(value)
    }

    trait Id {
        fn new(value: u64) -> Self;
    }

    macro_rules! ids {
        ($($type:ty),+ $(,)?) => {
            $(impl Id for $type {
                fn new(value: u64) -> Self {
                    <$type>::new(value).unwrap()
                }
            })+
        };
    }

    ids!(RootId, ScanId);

    #[test]
    fn picker_orders_fuzzy_relative_paths_and_filters_directories() {
        let mut picker = PickerState::default();
        picker.replace_entries([
            "zeta.md",
            "docs/README.MARKDOWN",
            "docs/reference.md",
            "docs/assets/",
            "notes.txt",
        ]);
        assert_eq!(
            picker
                .visible()
                .into_iter()
                .map(|entry| entry.relative_path)
                .collect::<Vec<_>>(),
            ["docs/README.MARKDOWN", "docs/reference.md", "zeta.md"]
        );
        picker.set_query("drm");
        assert_eq!(picker.selected(), Some("docs/README.MARKDOWN"));
    }

    #[test]
    fn progressive_batches_retain_selected_path() {
        let mut picker = PickerState::default();
        picker.replace_entries(["z.md", "b.md"]);
        picker.move_selection(1);
        assert_eq!(picker.selected(), Some("z.md"));
        picker
            .apply_batch(
                &DiscoveryBatch {
                    root: id(1),
                    scan: id(1),
                    entries: vec![DiscoveryEntry {
                        relative_path: "a.md".into(),
                    }],
                },
                id(1),
                id(1),
            )
            .unwrap();
        assert_eq!(picker.selected(), Some("z.md"));
    }

    #[test]
    fn fixed_shortcuts_and_disabled_history_are_deterministic() {
        let mut shell = ShellState::new();
        assert_eq!(shell.dispatch_shortcut(Shortcut::command('[')), None);
        assert_eq!(
            shell.dispatch_shortcut(Shortcut::command('p')),
            Some(ShellCommand::OpenPicker)
        );
        assert_eq!(shell.focus.owner(), FocusOwner::Picker);
        shell.close_focus(FocusOwner::Picker);
        assert_eq!(shell.focus.owner(), FocusOwner::Renderer);
        assert_eq!(shell.text_scale_percent, 100);
        shell.dispatch_shortcut(Shortcut::command('+'));
        assert_eq!(shell.text_scale_percent, 110);
    }

    #[test]
    fn focus_restores_prior_owner_and_startup_error_is_recoverable() {
        let mut shell = ShellState::new();
        shell.focus.open(FocusOwner::Search);
        shell.focus.open(FocusOwner::Palette);
        shell.close_focus(FocusOwner::Palette);
        assert_eq!(shell.focus.owner(), FocusOwner::Search);
        shell.startup_failed("/missing/readme.md".into(), "permission denied");
        let error = shell.startup_error.as_ref().unwrap();
        assert_eq!(error.failed_path, PathBuf::from("/missing/readme.md"));
        assert_eq!(
            shell.recover(RecoveryAction::Retry(error.failed_path.clone())),
            ShellCommand::RetryStartup(PathBuf::from("/missing/readme.md"))
        );
        assert!(shell.startup_error.is_none());
    }

    #[test]
    fn history_reports_availability() {
        let mut history = History::default();
        assert!(!history.can_go_back());
        history.push("/one.md".into());
        history.push("/two.md".into());
        assert!(history.can_go_back());
        assert_eq!(history.back(), Some(PathBuf::from("/one.md")));
        assert!(history.can_go_forward());
        assert_eq!(history.forward(), Some(PathBuf::from("/two.md")));
    }
}
