use std::{
    collections::VecDeque,
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::AtomicBool,
        mpsc::{Receiver, Sender, channel},
    },
    time::{Duration, Instant},
};

use gpui::{
    App, Application, Bounds, Context, FocusHandle, KeyDownEvent, Menu, MenuItem, Render,
    SystemMenuType, Task, Timer, Window, WindowAppearance, WindowBounds, WindowControlArea,
    WindowOptions, actions, div, point, prelude::*, px, size,
};

actions!(
    mdvr,
    [
        OpenFileAction,
        OpenFolderAction,
        ShowPreferences,
        ShowPicker,
        CycleToolbarVisibility,
        QuitApp,
        AboutMdvr
    ]
);

use crate::{
    LaunchPlan,
    contracts::{
        ActionMessage, ActionMessageEnvelope, ErrorCode, Generation, LocatorFallback,
        NavigationRequest, NavigationTarget, ResourceKind, ResourceReference, ResourceRequest,
        ResourceResult, ResourceResultValue, RootId, ScanId, SearchAction,
    },
    files::{
        DiscoveryEvent, DiscoveryScanner, LARGE_SOURCE_CONFIRM_BYTES, LoadError, LoadedSource,
        ReloadOutcome, load_source, spawn_reload_worker,
    },
    navigation::{LoadRequest, Locator, NavigationAction, NavigationState},
    platform::{
        EmbeddedWebView,
        bridge::{BridgeContext, BridgeMessage},
        choose_directory, choose_json_file, choose_markdown_file, confirm_large_document,
        confirm_outside_resource, confirm_remote_images, drain_bridge_messages, file_url_path,
        open_external_url, open_local_file,
        remote_fetch::fetch_image,
        remote_policy::{RemoteLimits, RemotePolicy},
        resource_policy::{ResourceAuthorization, ResourceDenied, ResourcePolicy},
        update_bridge_context,
    },
    preferences::{
        DisplayBounds, LaunchIntent, Preferences, ReadingLocator, ScrollbarVisibility,
        ToolbarIcons, WindowGeometry, conventional_path, load_or_default, resolve_launch, save,
    },
    theme::{
        AppearanceMode, Theme, ThemeFamily, ZedFonts, default_theme, import_file, load_zed_config,
    },
    ui::{FocusOwner, ShellCommand, ShellState},
};

#[derive(Clone, Debug)]
struct OpenRequest {
    path: PathBuf,
    ack: Option<PathBuf>,
}

static OPEN_PATHS: OnceLock<Mutex<VecDeque<OpenRequest>>> = OnceLock::new();

fn decode_open_request(path: PathBuf) -> Option<OpenRequest> {
    if path.extension().and_then(|value| value.to_str()) != Some("mdvr-request") {
        return Some(OpenRequest { path, ack: None });
    }
    let directory = path.parent()?;
    let safe_directory = directory
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.starts_with("mdvr-ipc-"))
        && fs::metadata(directory).ok()?.permissions().mode() & 0o077 == 0
        && fs::metadata(&path).ok()?.permissions().mode() & 0o077 == 0;
    if !safe_directory {
        return None;
    }
    let bytes = fs::read(&path).ok()?;
    let _ = fs::remove_file(&path);
    if bytes.is_empty() || bytes.len() > 4 * 1024 {
        return None;
    }
    let target = PathBuf::from(String::from_utf8(bytes).ok()?);
    if !target.is_absolute() {
        return None;
    }
    let ack = path.with_extension("ack");
    Some(OpenRequest {
        path: target,
        ack: Some(ack),
    })
}

fn enqueue_open_urls(urls: Vec<String>) {
    let requests = urls
        .iter()
        .filter_map(|url| file_url_path(url))
        .filter_map(decode_open_request)
        .collect::<Vec<_>>();
    let mut queue = OPEN_PATHS
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .expect("open-path queue poisoned");
    queue.extend(requests);
}

fn drain_open_paths() -> Vec<OpenRequest> {
    OPEN_PATHS
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .map(|mut queue| queue.drain(..).collect())
        .unwrap_or_default()
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum BridgeDispatchError {
    StaleContext {
        expected: BridgeContext,
        actual: BridgeContext,
    },
    Unfocused {
        action: &'static str,
        owner: FocusOwner,
    },
}

fn navigation_target_text(target: &NavigationTarget) -> String {
    match target {
        NavigationTarget::Anchor { value } => format!("#{value}"),
        NavigationTarget::Markdown { path }
        | NavigationTarget::Http { url: path }
        | NavigationTarget::Mailto { url: path }
        | NavigationTarget::LocalFile { path } => path.clone(),
    }
}

fn valid_external_http(url: &str) -> bool {
    if url.len() > 4 * 1024 || url.contains(char::is_control) {
        return false;
    }
    reqwest::Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

fn valid_mailto(url: &str) -> bool {
    url.len() <= 2 * 1024
        && url
            .get(..7)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("mailto:"))
        && !url.contains(char::is_control)
}

fn resource_mime(reference: &str) -> Option<String> {
    let extension = std::path::Path::new(reference)
        .extension()?
        .to_str()?
        .to_ascii_lowercase();
    Some(
        match extension.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            _ => return None,
        }
        .to_owned(),
    )
}

fn missing_dock_document(launch: &LaunchPlan, preferences: &Preferences) -> Option<PathBuf> {
    (launch.intent == LaunchIntent::Dock && launch.state.document.is_none())
        .then(|| preferences.last_document.clone())
        .flatten()
}

fn preference_locator(locator: &Locator) -> ReadingLocator {
    ReadingLocator {
        heading: locator.heading.clone(),
        block: locator.block.clone(),
        offset: locator.offset,
    }
}

fn contract_locator(locator: &Locator) -> crate::contracts::Locator {
    crate::contracts::Locator {
        heading: locator.heading.clone(),
        block: locator.block.clone(),
        offset: locator.offset,
        fallback: LocatorFallback::NearestHeading,
    }
}

fn navigation_locator(locator: &ReadingLocator) -> Locator {
    Locator {
        heading: locator.heading.clone(),
        block: locator.block.clone(),
        offset: locator.offset,
    }
}

fn action_context(action: &ActionMessageEnvelope) -> BridgeContext {
    BridgeContext {
        document: action.document,
        generation: action.generation,
    }
}

fn require_document_focus(
    shell: &ShellState,
    action: &'static str,
) -> Result<(), BridgeDispatchError> {
    if matches!(
        shell.focus.owner(),
        FocusOwner::Renderer | FocusOwner::Search
    ) {
        Ok(())
    } else {
        Err(BridgeDispatchError::Unfocused {
            action,
            owner: shell.focus.owner(),
        })
    }
}

/// Apply only native shell state. Renderer-side selection/search effects stay
/// in the focused document; native code never treats their payload as authority.
fn dispatch_bridge_action(
    shell: &mut ShellState,
    context: BridgeContext,
    action: &ActionMessageEnvelope,
) -> Result<(), BridgeDispatchError> {
    let actual = action_context(action);
    if actual != context {
        return Err(BridgeDispatchError::StaleContext {
            expected: context,
            actual,
        });
    }

    match &action.action {
        ActionMessage::Search(SearchAction::Open { .. }) => {
            shell.dispatch(ShellCommand::OpenSearch);
        }
        ActionMessage::Search(SearchAction::Close) => shell.close_focus(FocusOwner::Search),
        ActionMessage::Search(SearchAction::Next | SearchAction::Previous) => {
            require_document_focus(shell, "search")?;
        }
        ActionMessage::Copy(_) => {
            require_document_focus(shell, "copy")?;
            shell.dispatch(ShellCommand::CopyMarkdown);
        }
        ActionMessage::SelectAll => {
            require_document_focus(shell, "select_all")?;
            // DOM selection remains owned by focused renderer/input.
        }
        ActionMessage::Focus(owner) => {
            let owner = match owner {
                crate::contracts::FocusOwner::Shell => FocusOwner::Shell,
                crate::contracts::FocusOwner::Picker => FocusOwner::Picker,
                crate::contracts::FocusOwner::Search => FocusOwner::Search,
                crate::contracts::FocusOwner::Palette => FocusOwner::Palette,
                crate::contracts::FocusOwner::Renderer => FocusOwner::Renderer,
            };
            shell.focus.open(owner);
        }
        ActionMessage::Outline(action) => {
            shell.outline_visible = matches!(action, crate::contracts::OutlineAction::Open);
        }
        ActionMessage::TextScale(action) => shell.dispatch(match action {
            crate::contracts::TextScaleAction::Increase => ShellCommand::IncreaseTextSize,
            crate::contracts::TextScaleAction::Decrease => ShellCommand::DecreaseTextSize,
            crate::contracts::TextScaleAction::Reset => ShellCommand::ResetTextSize,
        }),
        ActionMessage::History(_) | ActionMessage::Theme(_) | ActionMessage::Open(_) => {}
        ActionMessage::CapturePosition(_) | ActionMessage::RestorePosition(_) => {
            unreachable!("router filters bridge actions")
        }
    }
    Ok(())
}

struct MdvrView {
    shell: ShellState,
    navigation: NavigationState,
    web_view: Option<EmbeddedWebView>,
    bridge_context: BridgeContext,
    reload_stop: Option<Arc<AtomicBool>>,
    reload_task: Option<Task<()>>,
    discovery_task: Option<Task<()>>,
    bridge_task: Option<Task<()>>,
    resource_policy: Option<ResourcePolicy>,
    preferences: Preferences,
    appearance_mode: Option<AppearanceMode>,
    picker_focus: FocusHandle,
    pending_open: VecDeque<OpenRequest>,
    pending_large: Option<(PathBuf, usize)>,
    pending_initial_locator: Option<ReadingLocator>,
    failed_path: Option<PathBuf>,
    picker_return: bool,
    startup_error: Option<String>,
    theme_family: Option<ThemeFamily>,
    zed_theme: Option<Theme>,
    zed_fonts: Option<ZedFonts>,
    loading_document: bool,
    initial_load_task: Option<Task<()>>,
    navigation_load_task: Option<Task<()>>,
    toolbar_visibility: ScrollbarVisibility,
    render_started: Option<Instant>,
    initialized: bool,
    remote_consent: Option<(crate::contracts::DocumentId, bool)>,
    remote_results: Receiver<ResourceResult>,
    remote_sender: Sender<ResourceResult>,
    remote_in_flight: usize,
}

impl MdvrView {
    fn cycle_toolbar_visibility(&mut self) {
        self.toolbar_visibility = match self.toolbar_visibility {
            ScrollbarVisibility::ShowOnScroll => ScrollbarVisibility::Show,
            ScrollbarVisibility::Show => ScrollbarVisibility::Hide,
            ScrollbarVisibility::Hide => ScrollbarVisibility::ShowOnScroll,
        };
        if let Some(web_view) = self.web_view.as_mut() {
            web_view.set_toolbar_visibility(self.toolbar_visibility);
        }
    }

    fn new(shell: ShellState, navigation: NavigationState, cx: &mut Context<Self>) -> Self {
        let (remote_sender, remote_results) = channel();
        let picker_focus = cx.focus_handle();
        let mut view = Self {
            shell,
            navigation,
            web_view: None,
            bridge_context: BridgeContext::default(),
            reload_stop: None,
            reload_task: None,
            discovery_task: None,
            bridge_task: None,
            resource_policy: None,
            preferences: Preferences::default(),
            appearance_mode: None,
            picker_focus,
            pending_open: VecDeque::new(),
            pending_large: None,
            pending_initial_locator: None,
            failed_path: None,
            picker_return: false,
            startup_error: None,
            theme_family: None,
            zed_theme: None,
            zed_fonts: None,
            loading_document: false,
            initial_load_task: None,
            navigation_load_task: None,
            toolbar_visibility: ScrollbarVisibility::ShowOnScroll,
            render_started: None,
            initialized: false,
            remote_consent: None,
            remote_results,
            remote_sender,
            remote_in_flight: 0,
        };
        let context = view
            .navigation
            .current()
            .map_or_else(BridgeContext::default, |document| BridgeContext {
                document: Some(document.document),
                generation: Some(document.generation),
            });
        view.document_committed(context);
        view
    }

    fn initialize(&mut self, window: &mut Window, launch: &LaunchPlan, cx: &mut Context<Self>) {
        self.bridge_task = Some(cx.spawn_in(window, async move |view, cx| {
            loop {
                Timer::after(Duration::from_millis(16)).await;
                if view
                    .update(cx, |view, cx| view.drain_bridge_messages(cx))
                    .is_err()
                {
                    return;
                }
                let opens = drain_open_paths();
                if !opens.is_empty()
                    && cx
                        .update(|window, app| {
                            view.update(app, |view, cx| {
                                view.pending_open.extend(opens);
                                view.process_next_open(window, cx);
                            })
                        })
                        .is_err()
                {
                    return;
                }
            }
        }));
        self.preferences = conventional_path()
            .map(|path| load_or_default(&path).preferences)
            .unwrap_or_default();
        self.initialized = true;
        self.shell.text_scale_percent = self.preferences.text_scale_percent;
        self.pending_initial_locator = launch.state.reading_locator.clone();
        let zed = self
            .preferences
            .use_zed_config
            .then(load_zed_config)
            .flatten();
        self.zed_theme = zed.as_ref().and_then(|config| config.theme.clone());
        self.zed_fonts = zed.map(|config| config.fonts);
        self.theme_family = self
            .preferences
            .theme_file
            .as_deref()
            .and_then(|path| import_file(path).ok())
            .or_else(|| {
                self.zed_theme.clone().map(|theme| ThemeFamily {
                    name: "Zed theme".into(),
                    members: vec![theme],
                })
            });
        if let Some(path) = missing_dock_document(launch, &self.preferences) {
            self.failed_path = Some(path.clone());
            self.report_error(format!(
                "Cannot restore {}: file does not exist",
                path.display()
            ));
        }
        if launch.state.document.is_none() {
            self.start_discovery(cx);
            window.focus(&self.picker_focus);
            return;
        }
        let path = launch
            .state
            .document
            .as_deref()
            .expect("document checked above");
        self.loading_document = true;
        let path = path.to_path_buf();
        let task = cx.background_spawn(async move { load_source(&path, false) });
        self.initial_load_task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = cx.update(|window, app| {
                view.update(app, |view, cx| view.finish_initial_load(result, window, cx))
            });
        }));
    }

    fn start_discovery(&mut self, cx: &mut Context<Self>) {
        let root = self
            .shell
            .root
            .clone()
            .expect("picker always has a browsing root");
        let root_id = RootId::new(1).expect("nonzero root");
        self.discovery_task = Some(cx.spawn(async move |view, cx| {
            let mut scan = 1_u64;
            loop {
                let scan_id = ScanId::new(scan).expect("nonzero scan");
                let discovery = DiscoveryScanner::new().spawn(root.clone(), root_id, scan_id);
                let mut collected = Vec::new();
                loop {
                    Timer::after(Duration::from_millis(25)).await;
                    match discovery.try_next() {
                        Ok(Some(DiscoveryEvent::Batch(batch))) => {
                            collected.extend(
                                batch
                                    .entries
                                    .iter()
                                    .map(|entry| entry.relative_path.clone()),
                            );
                            if scan == 1
                                && view
                                    .update(cx, |view, cx| {
                                        let _ =
                                            view.shell.picker.apply_batch(&batch, root_id, scan_id);
                                        cx.notify();
                                    })
                                    .is_err()
                            {
                                return;
                            }
                        }
                        Ok(Some(DiscoveryEvent::Complete(done))) => {
                            if view
                                .update(cx, |view, cx| {
                                    if scan > 1 {
                                        view.shell.picker.replace_entries(collected.clone());
                                    }
                                    let _ = view.shell.picker.complete(&done, root_id, scan_id);
                                    cx.notify();
                                })
                                .is_err()
                            {
                                return;
                            }
                            break;
                        }
                        Ok(Some(DiscoveryEvent::Error(error))) => {
                            let _ = view.update(cx, |view, cx| {
                                let _ = view.shell.picker.fail(&error, root_id, scan_id);
                                cx.notify();
                            });
                            return;
                        }
                        Ok(None) => {}
                        Err(error) => {
                            eprintln!("mdvr: discovery failed: {error}");
                            return;
                        }
                    }
                }
                Timer::after(Duration::from_secs(1)).await;
                scan = scan.checked_add(1).unwrap_or(1);
            }
        }));
    }

    fn handle_picker_key(&mut self, event: &KeyDownEvent, window: &Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.platform {
            match event.keystroke.key.as_str() {
                "o" if event.keystroke.modifiers.shift => {
                    if let Some(path) = choose_directory() {
                        self.open_directory(path, cx);
                    }
                }
                "o" => {
                    if let Some(path) = choose_markdown_file() {
                        self.pending_open.push_back(OpenRequest { path, ack: None });
                        cx.notify();
                    }
                }
                "p" if self.picker_return && !event.keystroke.modifiers.shift => {
                    self.restore_current_document(window, cx)
                }
                "r" if self.failed_path.is_some() => self.retry_failed_path(window, cx),
                _ => {}
            }
            return;
        }
        match event.keystroke.key.as_str() {
            "up" | "arrowup" => self.shell.picker.move_selection(-1),
            "down" | "arrowdown" => self.shell.picker.move_selection(1),
            "enter" => {
                if let Some(path) = self.shell.picker.selected().map(str::to_owned) {
                    self.open_picker_document(path, window, cx);
                }
                return;
            }
            "backspace" => {
                let mut query = self.shell.picker.query().to_owned();
                query.pop();
                self.shell.picker.set_query(query);
            }
            "escape" if self.picker_return => {
                self.restore_current_document(window, cx);
                return;
            }
            "escape" => self.shell.picker.set_query(""),
            _ => {
                let Some(text) = event.keystroke.key_char.as_deref().filter(|text| {
                    !text.is_empty() && text.chars().all(|character| !character.is_control())
                }) else {
                    return;
                };
                let mut query = self.shell.picker.query().to_owned();
                query.push_str(text);
                self.shell.picker.set_query(query);
            }
        }
        cx.notify();
    }

    fn finish_initial_load(
        &mut self,
        result: Result<LoadedSource, LoadError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.loading_document = false;
        self.initial_load_task = None;
        match result {
            Ok(source) => self.attach_initial_source(source, window, cx),
            Err(LoadError::NeedsConfirmation { path, bytes }) => {
                self.pending_large = Some((path, bytes));
                window.focus(&self.picker_focus);
                cx.notify();
            }
            Err(error) => {
                let path = match &error {
                    LoadError::Missing(path)
                    | LoadError::Unreadable(path)
                    | LoadError::NotAFile(path)
                    | LoadError::InvalidUtf8(path)
                    | LoadError::TooLarge { path, .. } => path.clone(),
                    LoadError::NeedsConfirmation { path, .. } => path.clone(),
                };
                self.failed_path = Some(path.clone());
                self.report_error(format!("Cannot load {}: {error}", path.display()));
                window.focus(&self.picker_focus);
                cx.notify();
            }
        }
    }

    fn attach_initial_source(
        &mut self,
        source: LoadedSource,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let path = source.path.clone();
        let Some(mut web_view) = EmbeddedWebView::attach(window) else {
            self.failed_path = Some(path.clone());
            self.report_error("Wry WebView attachment failed".into());
            return;
        };
        let _ = web_view.load_initial_document();
        let generation = Generation::new(1).expect("nonzero generation");
        if let Err(error) = web_view.load_document_source(&source.source, generation) {
            self.failed_path = Some(path.clone());
            self.report_error(format!("Cannot prepare {}: {error}", path.display()));
            return;
        }
        self.navigation.open_initial(source.clone());
        self.shell.current_document = Some(path.clone());
        self.failed_path = None;
        self.picker_return = false;
        self.startup_error = None;
        self.web_view = Some(web_view);
        self.watch_document(path, Some(source), cx);
        let current = self.navigation.current().expect("document was opened");
        self.document_committed(BridgeContext {
            document: Some(current.document),
            generation: Some(current.generation),
        });
        if let Some(locator) = self.pending_initial_locator.take() {
            let locator = navigation_locator(&locator);
            self.navigation.set_current_locator(locator.clone());
            if let (Some(web_view), Some(current)) =
                (self.web_view.as_mut(), self.navigation.current())
            {
                let _ = web_view.restore_locator(contract_locator(&locator), current.generation);
            }
        }
        self.appearance_mode = None;
        self.update_appearance(window);
        self.save_preferences();
        if let Some(web_view) = self.web_view.as_ref() {
            web_view.sync_frame();
            let _ = web_view.focus();
        }
        cx.notify();
    }

    fn confirm_large_file(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some((path, _)) = self.pending_large.take() else {
            return;
        };
        match load_source(&path, true) {
            Ok(source) => self.attach_initial_source(source, window, cx),
            Err(error) => {
                self.failed_path = Some(path.clone());
                self.report_error(format!("Cannot load {}: {error}", path.display()));
            }
        }
    }

    fn retry_failed_path(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(path) = self.failed_path.clone() else {
            return;
        };
        match load_source(&path, false) {
            Ok(source) => self.attach_initial_source(source, window, cx),
            Err(LoadError::NeedsConfirmation { path, bytes }) => {
                self.pending_large = Some((path, bytes));
                cx.notify();
            }
            Err(error) => self.report_error(format!("Cannot load {}: {error}", path.display())),
        }
    }

    fn open_picker_document(&mut self, relative: String, window: &Window, cx: &mut Context<Self>) {
        let Some(root) = self.shell.root.clone() else {
            return;
        };
        let path = root.join(relative);
        if self.picker_return {
            self.restore_current_document(window, cx);
            let Some(current) = self.navigation.current() else {
                return;
            };
            match self.navigation.request_navigation_from(
                &path.to_string_lossy(),
                current.generation,
                current.locator.clone(),
            ) {
                Ok(NavigationAction::Load(request)) => self.load_navigation(request, cx),
                Ok(_) => {}
                Err(error) => self.report_error(format!("Cannot open {}: {error}", path.display())),
            }
            return;
        }
        match load_source(&path, false) {
            Ok(source) => self.attach_initial_source(source, window, cx),
            Err(LoadError::NeedsConfirmation { path, bytes }) => {
                self.pending_large = Some((path, bytes));
                cx.notify();
            }
            Err(error) => {
                self.failed_path = Some(path.clone());
                self.report_error(format!("Cannot open {}: {error}", path.display()));
                cx.notify();
            }
        }
    }

    fn document_committed(&mut self, context: BridgeContext) {
        if self.remote_consent.map(|(document, _)| document) != context.document {
            self.remote_consent = None;
        }
        self.bridge_context = context;
        self.render_started = context.document.map(|_| Instant::now());
        update_bridge_context(context);
        self.resource_policy = self
            .shell
            .root
            .as_deref()
            .zip(self.navigation.current())
            .and_then(|(root, current)| {
                ResourcePolicy::new(root, &current.path, current.document, current.generation)
                    .map_err(|error| eprintln!("mdvr: cannot establish resource policy: {error:?}"))
                    .ok()
            });
        if let Some(web_view) = self.web_view.as_mut() {
            if let Err(error) =
                web_view.set_navigation_context(context.document, context.generation)
            {
                eprintln!("mdvr: cannot update renderer navigation context: {error}");
            }
            web_view.set_history_availability(
                self.navigation.can_go_back(),
                self.navigation.can_go_forward(),
            );
        }
    }

    fn import_theme(&mut self) {
        let Some(path) = choose_json_file() else {
            return;
        };
        match import_file(&path) {
            Ok(family) => {
                let Some(theme) = family.members.first() else {
                    return;
                };
                self.preferences.theme = Some(theme.name.clone());
                self.preferences.theme_file = Some(path);
                self.theme_family = Some(family);
                self.appearance_mode = None;
                self.save_preferences();
            }
            Err(error) => self.report_error(format!("Theme import failed: {error}")),
        }
    }

    fn update_appearance(&mut self, window: &Window) {
        let selected_theme = self
            .preferences
            .theme
            .as_deref()
            .and_then(|name| {
                self.theme_family
                    .as_ref()
                    .and_then(|family| family.member(name))
            })
            .or(self.zed_theme.as_ref());
        let mode = selected_theme.map_or_else(
            || match self.preferences.theme.as_deref() {
                Some("light") => AppearanceMode::Light,
                Some("dark") => AppearanceMode::Dark,
                _ => match window.appearance() {
                    WindowAppearance::Dark | WindowAppearance::VibrantDark => AppearanceMode::Dark,
                    WindowAppearance::Light | WindowAppearance::VibrantLight => {
                        AppearanceMode::Light
                    }
                },
            },
            |theme| theme.tokens.mode,
        );
        if let Some(web_view) = self.web_view.as_mut() {
            let names = self
                .theme_family
                .as_ref()
                .map(|family| {
                    family
                        .members
                        .iter()
                        .map(|theme| theme.name.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            web_view.set_theme_choices(&names, self.preferences.theme.as_deref());
            if let Some(fonts) = self.zed_fonts.as_ref() {
                web_view.set_fonts(fonts);
            }
            web_view.set_scrollbar_visibility(self.preferences.scrollbar_visibility);
            web_view.set_toolbar_icons(self.preferences.toolbar_icons);
            web_view.set_toolbar_visibility(self.toolbar_visibility);
        }
        if self.appearance_mode == Some(mode) {
            return;
        }
        let Some(generation) = self.bridge_context.generation else {
            return;
        };
        let tokens = selected_theme
            .map(|theme| theme.tokens.clone())
            .unwrap_or_else(|| default_theme(mode).tokens);
        let result = tokens
            .with_scale(self.preferences.text_scale_percent)
            .and_then(|tokens| tokens.as_revision_one())
            .map(|mut appearance| {
                if self.preferences.theme.is_none() {
                    appearance.mode = crate::contracts::AppearanceMode::System;
                }
                appearance
            });
        match (self.web_view.as_mut(), result) {
            (Some(web_view), Ok(appearance)) => {
                match web_view.apply_appearance(&appearance, generation) {
                    Ok(true) => self.appearance_mode = Some(mode),
                    Ok(false) => {}
                    Err(error) => eprintln!("mdvr: cannot apply appearance: {error}"),
                }
            }
            (_, Err(error)) => eprintln!("mdvr: invalid saved appearance: {error}"),
            _ => {}
        }
    }

    fn watch_document(
        &mut self,
        path: std::path::PathBuf,
        source: Option<LoadedSource>,
        cx: &mut Context<Self>,
    ) {
        if let Some(stop) = self.reload_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Release);
        }
        self.reload_task = None;
        let Some(generation) = self
            .navigation
            .current()
            .map(|document| document.generation)
        else {
            return;
        };
        let confirmed_large_file = source
            .as_ref()
            .is_some_and(|source| source.bytes > LARGE_SOURCE_CONFIRM_BYTES);
        match spawn_reload_worker(path.clone(), source, generation, confirmed_large_file) {
            Ok((receiver, stop)) => {
                self.reload_stop = Some(stop);
                self.reload_task = Some(cx.spawn(async move |view, cx| {
                    loop {
                        Timer::after(Duration::from_millis(25)).await;
                        let outcomes: Vec<_> = receiver.try_iter().collect();
                        if !outcomes.is_empty()
                            && view
                                .update(cx, |view, _| {
                                    for outcome in outcomes {
                                        view.apply_reload(outcome);
                                    }
                                })
                                .is_err()
                        {
                            return;
                        }
                    }
                }));
            }
            Err(error) => eprintln!("mdvr: cannot watch {}: {error}", path.display()),
        }
    }

    fn drain_bridge_messages(&mut self, cx: &mut Context<Self>) {
        if let Some(web_view) = self.web_view.as_mut() {
            web_view.flush_pending();
        }
        while let Ok(result) = self.remote_results.try_recv() {
            self.remote_in_flight = self.remote_in_flight.saturating_sub(1);
            if Some(result.document) == self.bridge_context.document
                && Some(result.generation) == self.bridge_context.generation
            {
                self.deliver_resource_result(result);
            }
        }
        for message in drain_bridge_messages() {
            match message {
                BridgeMessage::Action(action) => {
                    let focus_renderer = matches!(
                        action.action,
                        ActionMessage::Focus(crate::contracts::FocusOwner::Renderer)
                    );
                    if dispatch_bridge_action(&mut self.shell, self.bridge_context, &action).is_ok()
                    {
                        if matches!(action.action, ActionMessage::TextScale(_)) {
                            self.preferences.text_scale_percent = self.shell.text_scale_percent;
                            self.appearance_mode = None;
                            self.save_preferences();
                            cx.notify();
                        }
                        if let ActionMessage::Theme(theme) = &action.action {
                            match theme {
                                crate::contracts::ThemeAction::System => {
                                    self.preferences.theme = None
                                }
                                crate::contracts::ThemeAction::Light => {
                                    self.preferences.theme = Some("light".into())
                                }
                                crate::contracts::ThemeAction::Dark => {
                                    self.preferences.theme = Some("dark".into())
                                }
                                crate::contracts::ThemeAction::Import => self.import_theme(),
                                crate::contracts::ThemeAction::Named { name }
                                    if self
                                        .theme_family
                                        .as_ref()
                                        .and_then(|family| family.member(name))
                                        .is_some() =>
                                {
                                    self.preferences.theme = Some(name.clone());
                                }
                                crate::contracts::ThemeAction::Named { .. } => {}
                            }
                            if !matches!(theme, crate::contracts::ThemeAction::Import) {
                                self.appearance_mode = None;
                                self.save_preferences();
                            }
                            cx.notify();
                        }
                        match action.action {
                            ActionMessage::Open(crate::contracts::OpenAction::File) => {
                                if let Some(path) = choose_markdown_file() {
                                    self.pending_open.push_back(OpenRequest { path, ack: None });
                                    cx.notify();
                                }
                            }
                            ActionMessage::Open(crate::contracts::OpenAction::Folder) => {
                                if let Some(path) = choose_directory() {
                                    self.open_directory(path, cx);
                                }
                            }
                            ActionMessage::Open(crate::contracts::OpenAction::Picker) => {
                                self.open_document_picker(cx)
                            }
                            ActionMessage::History(crate::contracts::HistoryAction::Back) => {
                                self.go_back(cx)
                            }
                            ActionMessage::History(crate::contracts::HistoryAction::Forward) => {
                                self.go_forward(cx)
                            }
                            ActionMessage::History(crate::contracts::HistoryAction::Reload) => {
                                self.reload(cx)
                            }
                            _ => {}
                        }
                        if focus_renderer && let Some(web_view) = self.web_view.as_ref() {
                            let _ = web_view.focus();
                        }
                    }
                }
                BridgeMessage::Navigation(request) => self.dispatch_navigation(request, cx),
                BridgeMessage::Resource(request) => self.dispatch_resource(request),
                BridgeMessage::PositionCaptured(position) => {
                    if Some(position.document) != self.bridge_context.document
                        || Some(position.generation) != self.bridge_context.generation
                    {
                        continue;
                    }
                    let locator = Locator {
                        heading: position.locator.heading,
                        block: position.locator.block,
                        offset: position.locator.offset,
                    };
                    self.navigation.set_current_locator(locator.clone());
                    let preference = preference_locator(&locator);
                    if self.preferences.reading_locator.as_ref() != Some(&preference) {
                        self.preferences.reading_locator = Some(preference);
                        self.save_preferences();
                    }
                }
                BridgeMessage::RenderReady(ready) => {
                    if let Some(started) = self.render_started.take() {
                        eprintln!(
                            "mdvr: rendered generation {} in {:.1} ms",
                            ready.generation.get(),
                            started.elapsed().as_secs_f64() * 1000.0
                        );
                    }
                }
                BridgeMessage::RenderError(error) => {
                    self.render_started = None;
                    self.report_error(format!("Renderer error: {}", error.message));
                }
            }
        }
    }

    fn process_next_open(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(request) = self.pending_open.pop_front() else {
            return;
        };
        let accepted = self.open_received_document(request.path, window, cx);
        if let Some(ack) = request.ack {
            let _ = fs::write(ack, if accepted { "accepted" } else { "failed" });
        }
    }

    fn open_document_picker(&mut self, cx: &mut Context<Self>) {
        if self.navigation.current().is_none() || self.shell.root.is_none() {
            return;
        }
        if let Some(stop) = self.reload_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Release);
        }
        self.reload_task = None;
        self.web_view = None;
        self.shell.picker = Default::default();
        self.picker_return = true;
        self.start_discovery(cx);
        cx.notify();
    }

    fn restore_current_document(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(current) = self.navigation.current().cloned() else {
            return;
        };
        let Some(mut web_view) = EmbeddedWebView::attach(window) else {
            self.report_error("Wry WebView attachment failed".into());
            return;
        };
        let _ = web_view.load_initial_document();
        if web_view
            .load_document_source(&current.source, current.generation)
            .is_err()
        {
            self.report_error(format!("Cannot restore {}", current.path.display()));
            return;
        }
        self.web_view = Some(web_view);
        self.picker_return = false;
        self.discovery_task = None;
        self.document_committed(BridgeContext {
            document: Some(current.document),
            generation: Some(current.generation),
        });
        if let Some(web_view) = self.web_view.as_mut() {
            let _ =
                web_view.restore_locator(contract_locator(&current.locator), current.generation);
            let _ = web_view.focus();
        }
        self.watch_document(
            current.path.clone(),
            Some(LoadedSource {
                path: current.path,
                bytes: current.source.len(),
                source: current.source,
            }),
            cx,
        );
        self.appearance_mode = None;
        self.update_appearance(window);
        cx.notify();
    }

    fn open_directory(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if let Some(stop) = self.reload_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Release);
        }
        self.reload_task = None;
        self.web_view = None;
        self.navigation = NavigationState::new(path.clone());
        self.shell.root = Some(path);
        self.shell.current_document = None;
        self.shell.picker = Default::default();
        self.failed_path = None;
        self.picker_return = false;
        self.startup_error = None;
        self.document_committed(BridgeContext::default());
        self.start_discovery(cx);
        self.save_preferences();
        cx.notify();
    }

    fn open_received_document(
        &mut self,
        path: PathBuf,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if path.is_dir() {
            self.open_directory(path, cx);
            return true;
        }
        let Some(root) = path.parent().map(std::path::Path::to_owned) else {
            return false;
        };
        if let Some(current) = self.navigation.current() {
            let generation = current.generation;
            let locator = current.locator.clone();
            self.shell.root = Some(root);
            match self.navigation.request_navigation_from(
                &path.to_string_lossy(),
                generation,
                locator,
            ) {
                Ok(NavigationAction::Load(request)) => self.load_navigation(request, cx),
                Ok(_) => eprintln!("mdvr: Finder open did not resolve to Markdown"),
                Err(error) => eprintln!("mdvr: Finder open rejected: {error}"),
            }
        } else if let Some(name) = path.file_name() {
            self.shell.root = Some(root);
            self.open_picker_document(name.to_string_lossy().into_owned(), window, cx);
        }
        self.navigation
            .current()
            .is_some_and(|current| current.path == path)
            || self
                .pending_large
                .as_ref()
                .is_some_and(|(pending, _)| pending == &path)
    }

    fn dispatch_resource(&mut self, request: ResourceRequest) {
        if request.kind != ResourceKind::Image {
            self.deliver_resource_result(ResourceResult {
                request: request.request,
                resource: request.resource,
                document: request.document,
                generation: request.generation,
                result: ResourceResultValue::Denied {
                    code: ErrorCode::Unsupported,
                },
            });
            return;
        }

        let result = match (&mut self.resource_policy, &request.reference) {
            (Some(policy), ResourceReference::RelativePath { value }) => {
                let reference = std::path::Path::new(value);
                let authorization = match policy.authorize(reference) {
                    ResourceAuthorization::Denied(ResourceDenied::OutsideRoot) => policy
                        .consent_candidate(reference)
                        .ok()
                        .filter(|path| confirm_outside_resource(path))
                        .map_or(
                            ResourceAuthorization::Denied(ResourceDenied::OutsideRoot),
                            |_| policy.authorize_explicit(reference),
                        ),
                    authorization => authorization,
                };
                match authorization {
                    ResourceAuthorization::Allowed(grant) => policy
                        .read_granted_resource(request.document, request.generation, grant)
                        .ok()
                        .and_then(|bytes| resource_mime(value).map(|mime| (mime, bytes)))
                        .map_or(
                            ResourceResultValue::Denied {
                                code: ErrorCode::Denied,
                            },
                            |(mime, bytes)| ResourceResultValue::Bytes { mime, bytes },
                        ),
                    ResourceAuthorization::Denied(_) => ResourceResultValue::Denied {
                        code: ErrorCode::Denied,
                    },
                }
            }
            (Some(_), ResourceReference::RemoteUrl { value }) => {
                let valid = RemotePolicy::new(RemoteLimits::default())
                    .and_then(|policy| policy.authorize(value))
                    .is_ok();
                let consent = valid
                    && match self.remote_consent {
                        Some((document, true)) if document == request.document => true,
                        _ => {
                            let allowed = confirm_remote_images(value);
                            self.remote_consent = Some((request.document, allowed));
                            allowed
                        }
                    };
                if !consent {
                    ResourceResultValue::Denied {
                        code: ErrorCode::Denied,
                    }
                } else if self.remote_in_flight >= 4 {
                    ResourceResultValue::Denied {
                        code: ErrorCode::Internal,
                    }
                } else {
                    self.remote_in_flight += 1;
                    let sender = self.remote_sender.clone();
                    let url = value.clone();
                    std::thread::spawn(move || {
                        let result = RemotePolicy::new(RemoteLimits::default())
                            .map_err(Into::into)
                            .and_then(|policy| fetch_image(&policy, &url))
                            .map_or_else(
                                |error| {
                                    eprintln!("mdvr: remote image failed: {error:?}");
                                    ResourceResultValue::Denied {
                                        code: ErrorCode::Internal,
                                    }
                                },
                                |(mime, bytes)| ResourceResultValue::Bytes { mime, bytes },
                            );
                        let _ = sender.send(ResourceResult {
                            request: request.request,
                            resource: request.resource,
                            document: request.document,
                            generation: request.generation,
                            result,
                        });
                    });
                    return;
                }
            }
            _ => ResourceResultValue::Denied {
                code: ErrorCode::Unsupported,
            },
        };
        self.deliver_resource_result(ResourceResult {
            request: request.request,
            resource: request.resource,
            document: request.document,
            generation: request.generation,
            result,
        });
    }

    fn deliver_resource_result(&self, result: ResourceResult) {
        if let Some(web_view) = self.web_view.as_ref()
            && let Err(error) = web_view.deliver_resource(result)
        {
            eprintln!("mdvr: cannot deliver resource: {error}");
        }
    }

    fn dispatch_navigation(&mut self, request: NavigationRequest, cx: &mut Context<Self>) {
        let Some(current) = self.navigation.current() else {
            return;
        };
        if request.document != current.document || request.generation != current.generation {
            eprintln!("mdvr: ignored stale renderer navigation request");
            return;
        }
        let locator = current.locator.clone();
        let target = navigation_target_text(&request.target);
        match self
            .navigation
            .request_navigation_from(&target, request.generation, locator)
        {
            Ok(NavigationAction::Anchor { anchor, .. }) => {
                if let Some(web_view) = self.web_view.as_ref()
                    && let Err(error) = web_view.navigate_anchor(&anchor)
                {
                    eprintln!("mdvr: cannot navigate anchor: {error}");
                }
            }
            Ok(NavigationAction::Load(load)) => self.load_navigation(load, cx),
            Ok(NavigationAction::External(target)) => match target {
                NavigationTarget::Http { url } => {
                    if !valid_external_http(&url) || !open_external_url(&url) {
                        eprintln!("mdvr: external URL rejected");
                    }
                }
                NavigationTarget::Mailto { url } => {
                    if !valid_mailto(&url) || !open_external_url(&url) {
                        eprintln!("mdvr: mail URL rejected");
                    }
                }
                NavigationTarget::LocalFile { path } => {
                    let path = PathBuf::from(&path);
                    let image = resource_mime(&path.to_string_lossy()).is_some();
                    if !open_local_file(&path, !image) {
                        eprintln!("mdvr: local file rejected");
                    }
                }
                NavigationTarget::Anchor { .. } | NavigationTarget::Markdown { .. } => {
                    unreachable!("local targets are handled before external policy")
                }
            },
            Err(error) => eprintln!("mdvr: navigation rejected: {error}"),
        }
    }

    fn load_navigation(&mut self, request: LoadRequest, cx: &mut Context<Self>) {
        let path = request.path.clone();
        let task = cx.background_spawn(async move { load_source(&path, false) });
        self.navigation_load_task = Some(cx.spawn(async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                view.finish_navigation_load(request, result, cx)
            });
        }));
    }

    fn finish_navigation_load(
        &mut self,
        request: LoadRequest,
        result: Result<LoadedSource, LoadError>,
        cx: &mut Context<Self>,
    ) {
        self.navigation_load_task = None;
        let path = request.path.clone();
        let source = match result {
            Ok(source) => source,
            Err(LoadError::NeedsConfirmation { path, bytes })
                if confirm_large_document(&path, bytes) =>
            {
                match load_source(&path, true) {
                    Ok(source) => source,
                    Err(error) => {
                        let _ = self.navigation.fail_load(&request);
                        self.report_error(format!("Cannot open {}: {error}", path.display()));
                        return;
                    }
                }
            }
            Err(error) => {
                self.failed_path = Some(path.clone());
                let _ = self.navigation.fail_load(&request);
                self.report_error(format!("Cannot open {}: {error}", path.display()));
                return;
            }
        };
        let watched_source = source.clone();
        let accepted = self.web_view.as_mut().map_or(Ok(true), |web_view| {
            web_view.load_document_source(&source.source, request.generation)
        });
        match accepted {
            Ok(true) => match self.navigation.commit_load(request, source) {
                Ok(document) => {
                    self.commit_document(document);
                    self.watch_document(path, Some(watched_source), cx);
                }
                Err(error) => eprintln!("mdvr: stale navigation completion: {error}"),
            },
            Ok(false) => {
                let _ = self.navigation.fail_load(&request);
            }
            Err(error) => {
                let _ = self.navigation.fail_load(&request);
                eprintln!("mdvr: cannot prepare navigation: {error}");
            }
        }
    }

    fn commit_document(&mut self, document: crate::contracts::DocumentId) {
        let Some(current) = self.navigation.current() else {
            return;
        };
        let path = current.path.clone();
        let generation = current.generation;
        let locator = contract_locator(&current.locator);
        self.shell.current_document = Some(path);
        self.document_committed(BridgeContext {
            document: Some(document),
            generation: Some(generation),
        });
        if let Some(web_view) = self.web_view.as_mut()
            && let Err(error) = web_view.restore_locator(locator, generation)
        {
            eprintln!("mdvr: cannot restore reading position: {error}");
        }
        self.save_preferences();
    }

    fn capture_window_geometry(&mut self, window: &Window) {
        let bounds = window.bounds();
        let viewport = window.viewport_size();
        let geometry = WindowGeometry {
            x: (bounds.origin.x / px(1.0)).round() as i32,
            y: (bounds.origin.y / px(1.0)).round() as i32,
            width: (viewport.width / px(1.0)).round().max(0.0) as u32,
            height: (viewport.height / px(1.0)).round().max(0.0) as u32,
        };
        if geometry != self.preferences.window && self.preferences.set_window(geometry).is_ok() {
            self.save_preferences();
        }
    }

    fn report_error(&mut self, message: String) {
        eprintln!("mdvr: {message}");
        self.startup_error = Some(message.clone());
        if let Some(web_view) = self.web_view.as_ref()
            && let Err(error) = web_view.show_error(&message)
        {
            eprintln!("mdvr: cannot show error: {error}");
        }
    }

    fn save_preferences(&mut self) {
        self.preferences.browsing_root.clone_from(&self.shell.root);
        self.preferences
            .last_document
            .clone_from(&self.shell.current_document);
        if let Some(path) = conventional_path()
            && let Err(error) = save(&path, &self.preferences)
        {
            eprintln!("mdvr: cannot save preferences: {error}");
        }
    }

    fn go_back(&mut self, cx: &mut Context<Self>) {
        let locator = self
            .navigation
            .current()
            .map_or_else(Locator::start, |current| current.locator.clone());
        if let Ok(Some(request)) = self.navigation.go_back(locator) {
            self.load_navigation(request, cx);
        }
    }

    fn go_forward(&mut self, cx: &mut Context<Self>) {
        let locator = self
            .navigation
            .current()
            .map_or_else(Locator::start, |current| current.locator.clone());
        if let Ok(Some(request)) = self.navigation.go_forward(locator) {
            self.load_navigation(request, cx);
        }
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        if let Ok(request) = self.navigation.reload() {
            self.load_navigation(request, cx);
        }
    }

    fn apply_reload(&mut self, outcome: ReloadOutcome) {
        match outcome {
            ReloadOutcome::Ready { request, source } => {
                self.startup_error = None;
                if let Some(web_view) = self.web_view.as_ref() {
                    web_view.clear_error();
                }
                let Ok(navigation_request) = self.navigation.reload_at(request.generation) else {
                    eprintln!("mdvr: ignored stale reload generation");
                    return;
                };
                self.load_navigation_source(navigation_request, source);
            }
            ReloadOutcome::Failed { request, error } => {
                if let Ok(navigation_request) = self.navigation.reload_at(request.generation) {
                    let _ = self.navigation.fail_load(&navigation_request);
                }
                self.report_error(format!(
                    "Reload generation {} failed: {error}",
                    request.generation.get()
                ));
            }
        }
    }

    fn load_navigation_source(&mut self, request: LoadRequest, source: LoadedSource) {
        let accepted = self.web_view.as_mut().map_or(Ok(true), |web_view| {
            web_view.load_document_source(&source.source, request.generation)
        });
        match accepted {
            Ok(true) => match self.navigation.commit_load(request, source) {
                Ok(document) => self.commit_document(document),
                Err(error) => eprintln!("mdvr: stale reload completion: {error}"),
            },
            Ok(false) => {
                let _ = self.navigation.fail_load(&request);
            }
            Err(error) => {
                let _ = self.navigation.fail_load(&request);
                eprintln!("mdvr: cannot prepare reload: {error}");
            }
        }
    }
}

impl Drop for MdvrView {
    fn drop(&mut self) {
        if let Some(stop) = &self.reload_stop {
            stop.store(true, std::sync::atomic::Ordering::Release);
        }
        let _ = self.reload_task.take();
        let _ = self.discovery_task.take();
        let _ = self.initial_load_task.take();
        let _ = self.navigation_load_task.take();
        let _ = &self.bridge_task;
    }
}

impl Render for MdvrView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.process_next_open(window, cx);
        if self.initialized {
            self.update_appearance(window);
            self.capture_window_geometry(window);
        }
        if self.loading_document {
            return div().size_full().bg(gpui::rgb(0x202124)).into_any_element();
        }
        if let Some(web_view) = self.web_view.as_ref() {
            web_view.sync_frame();
            return div()
                .relative()
                .size_full()
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w(px(72.0))
                        .h(px(28.0))
                        .window_control_area(WindowControlArea::Drag),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px(340.0))
                        .right_0()
                        .h(px(28.0))
                        .window_control_area(WindowControlArea::Drag),
                )
                .into_any_element();
        }
        if let Some((path, bytes)) = self.pending_large.as_ref() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .gap_3()
                .bg(gpui::rgb(0x202124))
                .text_color(gpui::rgb(0xffffff))
                .child(format!(
                    "{} is {:.1} MiB",
                    path.display(),
                    *bytes as f64 / 1_048_576.0
                ))
                .child(
                    div()
                        .id("confirm-large-file")
                        .px_4()
                        .py_2()
                        .rounded_sm()
                        .cursor_pointer()
                        .bg(gpui::rgb(0x3c4043))
                        .child("Open full file")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.confirm_large_file(window, cx);
                        })),
                )
                .into_any_element();
        }
        window.focus(&self.picker_focus);
        let selected = self.shell.picker.selected().map(str::to_owned);
        let entries = self.shell.picker.visible();
        let can_retry = self.failed_path.is_some();
        div()
            .id("picker")
            .track_focus(&self.picker_focus)
            .on_key_down(cx.listener(|view, event, window, cx| {
                view.handle_picker_key(event, window, cx);
            }))
            .size_full()
            .flex()
            .flex_col()
            .p_4()
            .gap_2()
            .bg(gpui::rgb(0x202124))
            .text_color(gpui::rgb(0xf1f3f4))
            .child(
                div()
                    .text_color(gpui::rgb(0xb0b3b8))
                    .child(format!("Filter: {}", self.shell.picker.query())),
            )
            .when_some(self.startup_error.clone(), |view, error| {
                view.child(div().text_color(gpui::rgb(0xff8a80)).child(error))
            })
            .when(self.startup_error.is_some(), |view| {
                view.child(
                    div()
                        .flex()
                        .gap_2()
                        .when(can_retry, |row| {
                            row.child(
                                div()
                                    .id("retry-failed-path")
                                    .px_3()
                                    .py_2()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .bg(gpui::rgb(0x3c4043))
                                    .child("Retry")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.retry_failed_path(window, cx);
                                    })),
                            )
                        })
                        .child(
                            div()
                                .id("choose-file-after-error")
                                .px_3()
                                .py_2()
                                .rounded_sm()
                                .cursor_pointer()
                                .bg(gpui::rgb(0x3c4043))
                                .child("Choose file")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    if let Some(path) = choose_markdown_file() {
                                        view.pending_open
                                            .push_back(OpenRequest { path, ack: None });
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            div()
                                .id("browse-folder-after-error")
                                .px_3()
                                .py_2()
                                .rounded_sm()
                                .cursor_pointer()
                                .bg(gpui::rgb(0x3c4043))
                                .child("Browse folder")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    if let Some(path) = choose_directory() {
                                        view.open_directory(path, cx);
                                    }
                                })),
                        ),
                )
            })
            .child(
                div()
                    .id("picker-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(entries.into_iter().enumerate().map(|(index, entry)| {
                        let path = entry.relative_path;
                        let is_selected = selected.as_deref() == Some(path.as_str());
                        div()
                            .id(("picker-entry", index))
                            .px_3()
                            .py_2()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(|style| style.bg(gpui::rgb(0x303134)))
                            .when(is_selected, |style| style.bg(gpui::rgb(0x3c4043)))
                            .text_color(gpui::rgb(0xffffff))
                            .child(path.clone())
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.open_picker_document(path.clone(), window, cx);
                            }))
                    })),
            )
            .when(
                self.shell.picker.status().is_some() && self.shell.picker.visible().is_empty(),
                |view| {
                    view.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child("No Markdown files found")
                            .child(
                                div()
                                    .id("browse-empty-folder")
                                    .px_3()
                                    .py_2()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .bg(gpui::rgb(0x3c4043))
                                    .child("Choose folder")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        if let Some(path) = choose_directory() {
                                            view.open_directory(path, cx);
                                        }
                                    })),
                            ),
                    )
                },
            )
            .into_any_element()
    }
}

struct PreferencesView {
    preferences: Preferences,
    status: Option<String>,
}

impl PreferencesView {
    fn save(&mut self) {
        let Some(path) = conventional_path() else {
            self.status = Some("Preferences path unavailable".into());
            return;
        };
        self.status = Some(match save(&path, &self.preferences) {
            Ok(()) => "Saved".into(),
            Err(error) => format!("Save failed: {error}"),
        });
    }

    fn adjust_scale(&mut self, delta: i16) {
        let next = (i32::from(self.preferences.text_scale_percent) + i32::from(delta))
            .clamp(50, 300) as u16;
        let _ = self.preferences.set_text_scale(next);
        self.save();
    }

    fn toggle_zed_config(&mut self) {
        self.preferences.use_zed_config = !self.preferences.use_zed_config;
        self.save();
    }

    fn cycle_theme(&mut self) {
        self.preferences.theme = match self.preferences.theme.as_deref() {
            None => Some("light".into()),
            Some("light") => Some("dark".into()),
            _ => None,
        };
        self.save();
    }
}

impl Render for PreferencesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.preferences.theme.as_deref().unwrap_or("system");
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_6()
            .bg(gpui::rgb(0x202124))
            .text_color(gpui::rgb(0xf1f3f4))
            .child(div().text_xl().child("Preferences"))
            .child(div().child(format!("Theme: {theme}")))
            .child(div().child(format!(
                "Toolbar Icons: {}",
                match self.preferences.toolbar_icons {
                    ToolbarIcons::Icon => "Icon",
                    ToolbarIcons::IconAndText => "Icon + Text",
                    ToolbarIcons::TextOnly => "Text Only",
                }
            )))
            .child(div().child(format!(
                "Scrollbar: {}",
                match self.preferences.scrollbar_visibility {
                    ScrollbarVisibility::Hide => "Hidden",
                    ScrollbarVisibility::Show => "Always shown",
                    ScrollbarVisibility::ShowOnScroll => "Show on scroll",
                }
            )))
            .child(
                div()
                    .id("preferences-toolbar")
                    .px_3()
                    .py_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .bg(gpui::rgb(0x3c4043))
                    .child("Cycle toolbar icons")
                    .on_click(cx.listener(|view, _, _, _| {
                        view.preferences.toolbar_icons = match view.preferences.toolbar_icons {
                            ToolbarIcons::Icon => ToolbarIcons::IconAndText,
                            ToolbarIcons::IconAndText => ToolbarIcons::TextOnly,
                            ToolbarIcons::TextOnly => ToolbarIcons::Icon,
                        };
                        view.save();
                    })),
            )
            .child(
                div()
                    .id("preferences-scrollbar")
                    .px_3()
                    .py_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .bg(gpui::rgb(0x3c4043))
                    .child("Cycle scrollbar visibility")
                    .on_click(cx.listener(|view, _, _, _| {
                        view.preferences.scrollbar_visibility =
                            match view.preferences.scrollbar_visibility {
                                ScrollbarVisibility::ShowOnScroll => ScrollbarVisibility::Show,
                                ScrollbarVisibility::Show => ScrollbarVisibility::Hide,
                                ScrollbarVisibility::Hide => ScrollbarVisibility::ShowOnScroll,
                            };
                        view.save();
                    })),
            )
            .child(
                div()
                    .id("preferences-zed-config")
                    .px_3()
                    .py_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .bg(gpui::rgb(0x3c4043))
                    .child(format!(
                        "Use Zed theme/fonts: {}",
                        if self.preferences.use_zed_config {
                            "On"
                        } else {
                            "Off"
                        }
                    ))
                    .on_click(cx.listener(|view, _, _, _| view.toggle_zed_config())),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(format!(
                        "Text scale: {}%",
                        self.preferences.text_scale_percent
                    ))
                    .child(
                        div()
                            .id("preferences-scale-down")
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .bg(gpui::rgb(0x3c4043))
                            .child("−")
                            .on_click(cx.listener(|view, _, _, _| view.adjust_scale(-10))),
                    )
                    .child(
                        div()
                            .id("preferences-scale-up")
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .bg(gpui::rgb(0x3c4043))
                            .child("+")
                            .on_click(cx.listener(|view, _, _, _| view.adjust_scale(10))),
                    ),
            )
            .child(
                div()
                    .id("preferences-theme")
                    .px_3()
                    .py_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .bg(gpui::rgb(0x3c4043))
                    .child("Cycle theme")
                    .on_click(cx.listener(|view, _, _, _| view.cycle_theme())),
            )
            .child(
                self.status
                    .clone()
                    .unwrap_or_else(|| "Changes save automatically".into()),
            )
    }
}

fn active_mdvr_window(cx: &mut App) -> Option<gpui::WindowHandle<MdvrView>> {
    cx.active_window()
        .and_then(|window| window.downcast::<MdvrView>())
        .or_else(|| {
            cx.windows()
                .into_iter()
                .find_map(|window| window.downcast::<MdvrView>())
        })
}

fn open_file_action(_: &OpenFileAction, cx: &mut App) {
    let Some(path) = choose_markdown_file() else {
        return;
    };
    let Some(window) = active_mdvr_window(cx) else {
        return;
    };
    let _ = window.update(cx, |view, _, cx| {
        view.pending_open.push_back(OpenRequest { path, ack: None });
        cx.notify();
    });
}

fn cycle_toolbar_visibility_action(_: &CycleToolbarVisibility, cx: &mut App) {
    if let Some(window) = active_mdvr_window(cx) {
        let _ = window.update(cx, |view, _, _| view.cycle_toolbar_visibility());
    }
}

fn show_picker_action(_: &ShowPicker, cx: &mut App) {
    if let Some(window) = active_mdvr_window(cx) {
        let _ = window.update(cx, |view, _, cx| view.open_document_picker(cx));
    }
}

fn open_folder_action(_: &OpenFolderAction, cx: &mut App) {
    let Some(path) = choose_directory() else {
        return;
    };
    let Some(window) = active_mdvr_window(cx) else {
        return;
    };
    let _ = window.update(cx, |view, _, cx| view.open_directory(path, cx));
}

fn show_preferences(_: &ShowPreferences, cx: &mut App) {
    let preferences = conventional_path()
        .map(|path| load_or_default(&path).preferences)
        .unwrap_or_default();
    let _ = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(300.0), px(200.0)),
                size: size(px(420.0), px(280.0)),
            })),
            is_resizable: true,
            ..WindowOptions::default()
        },
        move |_, cx| {
            cx.new(|_| PreferencesView {
                preferences,
                status: None,
            })
        },
    );
}

fn quit_app(_: &QuitApp, cx: &mut App) {
    cx.quit();
}

fn about_mdvr(_: &AboutMdvr, _cx: &mut App) {
    eprintln!("mdvr {}", env!("CARGO_PKG_VERSION"));
}

pub(crate) fn dock_launch(fallback: &LaunchPlan) -> LaunchPlan {
    let preferences = conventional_path()
        .map(|path| load_or_default(&path).preferences)
        .unwrap_or_default();
    let available = preferences
        .last_document
        .as_deref()
        .is_some_and(std::path::Path::is_file);
    let state = resolve_launch(LaunchIntent::Dock, None, &preferences, available)
        .expect("validated Dock restore");
    let picker_root = state
        .browsing_root
        .clone()
        .or_else(|| {
            state
                .document
                .as_deref()
                .and_then(std::path::Path::parent)
                .map(std::path::Path::to_owned)
        })
        .unwrap_or_else(|| fallback.picker_root.clone());
    LaunchPlan {
        intent: LaunchIntent::Dock,
        state,
        picker_root,
        explicit: false,
    }
}

fn open_mdvr_window(cx: &mut App, launch: LaunchPlan, announce: bool) {
    let geometry = conventional_path()
        .map(|path| load_or_default(&path).preferences.window)
        .unwrap_or_default();
    let displays = cx
        .displays()
        .into_iter()
        .map(|display| {
            let bounds = display.bounds();
            DisplayBounds {
                x: (bounds.origin.x / px(1.0)).round() as i32,
                y: (bounds.origin.y / px(1.0)).round() as i32,
                width: (bounds.size.width / px(1.0)).round().max(0.0) as u32,
                height: (bounds.size.height / px(1.0)).round().max(0.0) as u32,
            }
        })
        .collect::<Vec<_>>();
    let geometry = geometry.restore_on(&displays);
    let bounds = Bounds {
        origin: point(px(geometry.x as f32), px(geometry.y as f32)),
        size: size(px(geometry.width as f32), px(geometry.height as f32)),
    };
    let launch_for_window = launch.clone();
    match cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("mdvr".into()),
                appears_transparent: true,
                traffic_light_position: None,
            }),
            is_resizable: true,
            ..WindowOptions::default()
        },
        move |_window, cx| {
            let mut shell = ShellState::new();
            shell.root = Some(launch_for_window.picker_root.clone());
            shell.current_document = launch_for_window.state.document.clone();
            let navigation = NavigationState::new(launch_for_window.picker_root.clone());
            cx.new(|cx| MdvrView::new(shell, navigation, cx))
        },
    ) {
        Ok(window) => {
            let _ = window.update(cx, |view, native_window, cx| {
                view.initialize(native_window, &launch, cx);
                cx.notify();
            });
            if announce && launch.explicit {
                let path = launch
                    .state
                    .document
                    .as_ref()
                    .or(launch.state.browsing_root.as_ref());
                if let Some(path) = path {
                    println!("mdvr: accepted {}", path.display());
                }
            }
            cx.activate(true);
        }
        Err(error) => {
            eprintln!("mdvr: launch failed: {error}");
            if announce {
                std::process::exit(1);
            }
        }
    }
}

pub fn run(launch: LaunchPlan) {
    let reopen_launch = launch.clone();
    let application = Application::new();
    application.on_open_urls(enqueue_open_urls);
    application.on_reopen(move |cx| {
        if cx.windows().is_empty() {
            open_mdvr_window(cx, dock_launch(&reopen_launch), false);
        }
    });
    application.run(move |cx: &mut App| {
        cx.on_action(open_file_action);
        cx.on_action(open_folder_action);
        cx.on_action(show_picker_action);
        cx.on_action(cycle_toolbar_visibility_action);
        cx.on_action(show_preferences);
        cx.on_action(quit_app);
        cx.on_action(about_mdvr);
        cx.bind_keys([
            gpui::KeyBinding::new("cmd-o", OpenFileAction, None),
            gpui::KeyBinding::new("cmd-shift-o", OpenFolderAction, None),
            gpui::KeyBinding::new("cmd-p", ShowPicker, None),
            gpui::KeyBinding::new("cmd-shift-b", CycleToolbarVisibility, None),
            gpui::KeyBinding::new("cmd-,", ShowPreferences, None),
            gpui::KeyBinding::new("cmd-q", QuitApp, None),
        ]);
        cx.set_menus(vec![
            Menu {
                name: "mdvr".into(),
                items: vec![
                    MenuItem::action("About mdvr", AboutMdvr),
                    MenuItem::action("Preferences…", ShowPreferences),
                    MenuItem::os_submenu("Services", SystemMenuType::Services),
                    MenuItem::separator(),
                    MenuItem::action("Quit mdvr", QuitApp),
                ],
            },
            Menu {
                name: "File".into(),
                items: vec![
                    MenuItem::action("Open File…", OpenFileAction),
                    MenuItem::action("Open Folder…", OpenFolderAction),
                    MenuItem::separator(),
                    MenuItem::action("Browse Files", ShowPicker),
                    MenuItem::action("Cycle Toolbar Visibility", CycleToolbarVisibility),
                ],
            },
        ]);
        open_mdvr_window(cx, launch, true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{DocumentId, Generation, RequestId};

    fn action(
        document: Option<DocumentId>,
        generation: Option<Generation>,
        action: ActionMessage,
    ) -> ActionMessageEnvelope {
        ActionMessageEnvelope {
            request: RequestId::new(1).unwrap(),
            document,
            generation,
            action,
        }
    }

    #[test]
    fn missing_cold_dock_document_is_reported() {
        let path = PathBuf::from("/saved/missing.md");
        let plan = LaunchPlan {
            intent: LaunchIntent::Dock,
            state: crate::preferences::LaunchState {
                browsing_root: Some(PathBuf::from("/saved")),
                document: None,
                reading_locator: None,
            },
            picker_root: PathBuf::from("/saved"),
            explicit: false,
        };
        let preferences = Preferences {
            last_document: Some(path.clone()),
            ..Preferences::default()
        };
        assert_eq!(missing_dock_document(&plan, &preferences), Some(path));
    }

    #[test]
    fn explicit_browser_links_allow_localhost_but_reject_other_schemes() {
        assert!(valid_external_http("http://localhost:3000/docs"));
        assert!(valid_external_http("https://example.com/docs"));
        assert!(!valid_external_http("javascript:alert(1)"));
        assert!(!valid_external_http("https://example.com/\nheader"));
    }

    #[test]
    fn mailto_validation_is_bounded_and_blocks_control_characters() {
        assert!(valid_mailto("MAILTO:reader@example.com"));
        assert!(!valid_mailto(
            "mailto:reader@example.com\r\nBcc:x@example.com"
        ));
        assert!(!valid_mailto(&format!("mailto:{}", "x".repeat(2048))));
    }

    #[test]
    fn launch_request_file_is_private_bounded_and_consumed() {
        let directory = std::env::temp_dir().join(format!("mdvr-ipc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let request = directory.join("request.test.mdvr-request");
        fs::write(&request, "/tmp/document.md").unwrap();
        fs::set_permissions(&request, fs::Permissions::from_mode(0o600)).unwrap();

        let decoded = decode_open_request(request.clone()).unwrap();
        assert_eq!(decoded.path, PathBuf::from("/tmp/document.md"));
        assert_eq!(decoded.ack, Some(directory.join("request.test.ack")));
        assert!(!request.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn resource_mime_allows_only_static_image_formats() {
        assert_eq!(
            resource_mime("diagram.SVG").as_deref(),
            Some("image/svg+xml")
        );
        assert_eq!(resource_mime("photo.jpeg").as_deref(), Some("image/jpeg"));
        assert_eq!(resource_mime("animated.gif").as_deref(), Some("image/gif"));
        assert_eq!(resource_mime("payload.html"), None);
    }

    #[test]
    fn dispatches_text_scale_into_native_state() {
        let mut shell = ShellState::new();
        dispatch_bridge_action(
            &mut shell,
            BridgeContext::default(),
            &action(
                None,
                None,
                ActionMessage::TextScale(crate::contracts::TextScaleAction::Increase),
            ),
        )
        .unwrap();
        assert_eq!(shell.text_scale_percent, 110);
    }

    #[test]
    fn dispatches_search_and_focus_into_shell_state() {
        let mut shell = ShellState::new();
        let context = BridgeContext::default();
        dispatch_bridge_action(
            &mut shell,
            context,
            &action(
                None,
                None,
                ActionMessage::Search(SearchAction::Open {
                    query: "needle".into(),
                    case_sensitive: false,
                }),
            ),
        )
        .unwrap();
        assert_eq!(shell.focus.owner(), FocusOwner::Search);

        dispatch_bridge_action(
            &mut shell,
            context,
            &action(
                None,
                None,
                ActionMessage::Focus(crate::contracts::FocusOwner::Renderer),
            ),
        )
        .unwrap();
        assert_eq!(shell.focus.owner(), FocusOwner::Renderer);
    }

    #[test]
    fn copy_and_select_all_require_renderer_or_input_focus() {
        let mut shell = ShellState::new();
        shell.focus.open(FocusOwner::Picker);
        let context = BridgeContext::default();
        let copy = action(
            None,
            None,
            ActionMessage::Copy(crate::contracts::CopyAction::Rendered),
        );
        assert!(matches!(
            dispatch_bridge_action(&mut shell, context, &copy),
            Err(BridgeDispatchError::Unfocused {
                action: "copy",
                owner: FocusOwner::Picker
            })
        ));

        shell.close_focus(FocusOwner::Picker);
        assert!(
            dispatch_bridge_action(
                &mut shell,
                context,
                &action(None, None, ActionMessage::SelectAll)
            )
            .is_ok()
        );
    }

    #[test]
    fn stale_document_context_is_rejected_before_dispatch() {
        let mut shell = ShellState::new();
        let current = BridgeContext {
            document: Some(DocumentId::new(2).unwrap()),
            generation: Some(Generation::new(4).unwrap()),
        };
        let stale = action(
            Some(DocumentId::new(1).unwrap()),
            Some(Generation::new(3).unwrap()),
            ActionMessage::Search(SearchAction::Open {
                query: "stale".into(),
                case_sensitive: false,
            }),
        );

        assert!(matches!(
            dispatch_bridge_action(&mut shell, current, &stale),
            Err(BridgeDispatchError::StaleContext { .. })
        ));
        assert_eq!(shell.focus.owner(), FocusOwner::Renderer);
    }
}
