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
    time::{Duration, Instant, SystemTime},
};

use gpui::{
    App, Application, Bounds, Context, FocusHandle, FontWeight, KeyDownEvent, Menu, MenuItem,
    MouseButton, OsAction, PathPromptOptions, Render, Rgba, Subscription, SystemMenuType, Task,
    Timer, Window, WindowAppearance, WindowBounds, WindowOptions, actions, div, point, prelude::*,
    px, size,
};

actions!(
    mdvr,
    [
        OpenFileAction,
        OpenFolderAction,
        ShowPreferences,
        ShowPicker,
        CloseWindow,
        Cut,
        Copy,
        Paste,
        SelectAll,
        Undo,
        Redo,
        QuitApp,
        AboutMdvr
    ]
);

#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = mdvr, no_json)]
struct OpenRecent {
    path: String,
}

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
        EmbeddedWebView, PreferencesMessage, begin_window_drag,
        bridge::{BridgeContext, BridgeMessage},
        confirm_large_document, confirm_outside_resource, confirm_remote_images, file_url_path,
        install_close_shortcut, open_external_url, open_local_file, perform_redo, perform_undo,
        remote_fetch::fetch_image,
        remote_policy::{RemoteLimits, RemotePolicy},
        resource_policy::{ResourceAuthorization, ResourceDenied, ResourcePolicy},
        set_titlebar_controls_visible, set_window_appearance, set_window_background_draggable,
        show_about,
    },
    preferences::{
        DisplayBounds, LaunchIntent, Preferences, ReadingLocator, WindowGeometry,
        conventional_path, load_or_default, resolve_launch, save,
    },
    theme::{
        AppearanceMode, Theme, ThemeFamily, ZedFonts, available_family, default_theme, import_file,
        load_zed_config,
    },
    ui::{FocusOwner, ShellCommand, ShellState},
};

#[derive(Clone, Debug)]
struct OpenRequest {
    path: PathBuf,
    ack: Option<PathBuf>,
}

static OPEN_PATHS: OnceLock<Mutex<VecDeque<OpenRequest>>> = OnceLock::new();
const MAIN_TITLEBAR_HEIGHT: f64 = 38.0;

fn ui_font_weight(value: &str) -> Option<FontWeight> {
    Some(match value.to_ascii_lowercase().as_str() {
        "thin" => FontWeight::THIN,
        "extra_light" | "extralight" => FontWeight::EXTRA_LIGHT,
        "light" => FontWeight::LIGHT,
        "normal" | "regular" => FontWeight::NORMAL,
        "medium" => FontWeight::MEDIUM,
        "semibold" | "semi_bold" => FontWeight::SEMIBOLD,
        "bold" => FontWeight::BOLD,
        "extra_bold" | "extrabold" => FontWeight::EXTRA_BOLD,
        "black" => FontWeight::BLACK,
        value if value.parse::<f32>().is_ok() => FontWeight(value.parse().ok()?),
        _ => return None,
    })
}

fn main_window_frame(titlebar: gpui::AnyElement, content: gpui::AnyElement) -> gpui::AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(titlebar)
        .child(div().flex_1().min_h_0().child(content))
        .into_any_element()
}

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

#[derive(Clone)]
struct SharedPreferences {
    preferences: Preferences,
    theme_family: ThemeFamily,
    zed_theme: Option<Theme>,
    zed_fonts: Option<ZedFonts>,
}

impl SharedPreferences {
    fn new(preferences: Preferences) -> Self {
        let zed = preferences.use_zed_config.then(load_zed_config).flatten();
        let zed_theme = zed.as_ref().and_then(|config| config.theme.clone());
        let zed_fonts = zed.map(|config| config.fonts);
        let mut theme_family = available_family();
        if let Some(imported) = preferences
            .theme_file
            .as_deref()
            .and_then(|path| import_file(path).ok())
        {
            for theme in imported.members {
                if !theme_family
                    .members
                    .iter()
                    .any(|member| member.name == theme.name)
                {
                    theme_family.members.push(theme);
                }
            }
        }
        theme_family.sort_members();
        Self {
            preferences,
            theme_family,
            zed_theme,
            zed_fonts,
        }
    }
}

impl gpui::Global for SharedPreferences {}

#[derive(Clone, Copy)]
struct UiPalette {
    dark: bool,
    background: Rgba,
    foreground: Rgba,
    control: Rgba,
    accent: Rgba,
    selection: Rgba,
}

fn theme_color(value: &str, fallback: u32) -> Rgba {
    value
        .strip_prefix('#')
        .and_then(|value| value.get(..6))
        .and_then(|value| u32::from_str_radix(value, 16).ok())
        .map_or_else(|| gpui::rgb(fallback), gpui::rgb)
}

fn ui_palette(preferences: &Preferences, shared: &SharedPreferences, window: &Window) -> UiPalette {
    let selected = if preferences.theme.is_none() {
        shared.zed_theme.as_ref()
    } else {
        preferences
            .theme
            .as_deref()
            .and_then(|name| shared.theme_family.member(name))
    };
    let mode = selected.map_or_else(
        || match preferences.theme.as_deref() {
            Some("light") => AppearanceMode::Light,
            Some("dark") => AppearanceMode::Dark,
            _ => match window.appearance() {
                WindowAppearance::Dark | WindowAppearance::VibrantDark => AppearanceMode::Dark,
                WindowAppearance::Light | WindowAppearance::VibrantLight => AppearanceMode::Light,
            },
        },
        |theme| theme.tokens.mode,
    );
    let tokens = selected
        .map(|theme| theme.tokens.clone())
        .unwrap_or_else(|| default_theme(mode).tokens);
    let background = theme_color(&tokens.reader_background, 0x202124);
    let accent = theme_color(&tokens.accent, 0x7aa2f7);
    UiPalette {
        dark: mode == AppearanceMode::Dark,
        background,
        foreground: theme_color(&tokens.reader_foreground, 0xf1f3f4),
        control: theme_color(&tokens.code_background, 0x3c4043),
        accent,
        selection: Rgba {
            r: background.r * 0.82 + accent.r * 0.18,
            g: background.g * 0.82 + accent.g * 0.18,
            b: background.b * 0.82 + accent.b * 0.18,
            a: 1.0,
        },
    }
}

fn relative_age(value: u64, unit: &str) -> String {
    format!("{value} {unit}{} ago", if value == 1 { "" } else { "s" })
}

fn relative_mtime(root: &std::path::Path, relative: &str) -> String {
    let elapsed = root
        .join(relative)
        .metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok());
    let Some(elapsed) = elapsed else {
        return "modified time unavailable".into();
    };
    let seconds = elapsed.as_secs();
    match seconds {
        0..=59 => "just now".into(),
        60..=3_599 => relative_age(seconds / 60, "minute"),
        3_600..=86_399 => relative_age(seconds / 3_600, "hour"),
        86_400..=2_591_999 => relative_age(seconds / 86_400, "day"),
        2_592_000..=31_535_999 => relative_age(seconds / 2_592_000, "month"),
        _ => relative_age(seconds / 31_536_000, "year"),
    }
}

struct TextTooltip {
    text: &'static str,
    palette: UiPalette,
}

impl Render for TextTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .top(px(-12.0))
            .h(px(18.0))
            .px_2()
            .flex()
            .items_center()
            .rounded_sm()
            .bg(self.palette.control)
            .text_color(self.palette.foreground)
            .text_xs()
            .child(self.text)
    }
}

fn publish_preferences(cx: &mut App, preferences: &Preferences) {
    let shared = SharedPreferences::new(preferences.clone());
    cx.update_global::<SharedPreferences, _>(|state, _| *state = shared);
    if let Some(path) = conventional_path()
        && let Err(error) = save(&path, preferences)
    {
        eprintln!("mdvr: cannot save preferences: {error}");
    }
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
    appearance_task: Option<Task<()>>,
    resource_policy: Option<ResourcePolicy>,
    preferences: Preferences,
    #[allow(dead_code)]
    preferences_subscription: Subscription,
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
    activation_subscription: Option<Subscription>,
    render_started: Option<Instant>,
    initialized: bool,
    remote_consent: Option<(crate::contracts::DocumentId, bool)>,
    remote_results: Receiver<ResourceResult>,
    remote_sender: Sender<ResourceResult>,
    remote_in_flight: usize,
}

impl MdvrView {
    fn new(shell: ShellState, navigation: NavigationState, cx: &mut Context<Self>) -> Self {
        let (remote_sender, remote_results) = channel();
        let picker_focus = cx.focus_handle();
        let preferences_subscription = cx.observe_global::<SharedPreferences>(|view, cx| {
            let shared = cx.global::<SharedPreferences>().clone();
            if shared.preferences != view.preferences {
                view.preferences = shared.preferences;
                view.theme_family = Some(shared.theme_family);
                view.zed_theme = shared.zed_theme;
                view.zed_fonts = shared.zed_fonts;
                if let Some(web_view) = view.web_view.as_mut() {
                    web_view.set_fonts(view.zed_fonts.as_ref().unwrap_or(&ZedFonts::default()));
                }
                view.appearance_mode = None;
                cx.notify();
            }
        });
        let mut view = Self {
            shell,
            navigation,
            web_view: None,
            bridge_context: BridgeContext::default(),
            reload_stop: None,
            reload_task: None,
            discovery_task: None,
            bridge_task: None,
            appearance_task: None,
            resource_policy: None,
            preferences: cx.global::<SharedPreferences>().preferences.clone(),
            preferences_subscription,
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
            activation_subscription: None,
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
        self.activation_subscription = Some(cx.observe_window_activation(window, |_, _, cx| {
            cx.notify();
        }));
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
        self.appearance_task = Some(cx.spawn_in(window, async move |view, cx| {
            loop {
                Timer::after(Duration::from_millis(250)).await;
                if view.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        }));
        let shared = cx.global::<SharedPreferences>().clone();
        self.preferences = shared.preferences;
        self.theme_family = Some(shared.theme_family);
        self.zed_theme = shared.zed_theme;
        self.zed_fonts = shared.zed_fonts;
        self.initialized = true;
        self.shell.text_scale_percent = self.preferences.text_scale_percent;
        self.pending_initial_locator = launch.state.reading_locator.clone();
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
                "p" if self.picker_return && !event.keystroke.modifiers.shift => {
                    self.restore_current_document(window, cx)
                }
                "r" if self.failed_path.is_some() => self.retry_failed_path(window, cx),
                _ => cx.propagate(),
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
        let Some(mut web_view) = EmbeddedWebView::attach(window, MAIN_TITLEBAR_HEIGHT) else {
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
        cx.add_recent_document(&path);
        self.preferences.record_recent(path.clone());
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
            web_view.sync_frame(MAIN_TITLEBAR_HEIGHT);
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
        if let Some(web_view) = self.web_view.as_mut()
            && let Err(error) =
                web_view.set_navigation_context(context.document, context.generation)
        {
            eprintln!("mdvr: cannot update renderer navigation context: {error}");
        }
    }

    fn prompt_for_picker_document(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open Markdown".into()),
        });
        cx.spawn(async move |view, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                view.pending_open.push_back(OpenRequest { path, ack: None });
                cx.notify();
            });
        })
        .detach();
    }

    fn prompt_for_picker_directory(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Folder".into()),
        });
        cx.spawn(async move |view, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = view.update(cx, |view, cx| view.open_directory(path, cx));
        })
        .detach();
    }

    fn import_theme(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import Theme".into()),
        });
        cx.spawn(async move |view, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = view.update(cx, |view, cx| match import_file(&path) {
                Ok(family) => {
                    let Some(theme) = family.members.first() else {
                        return;
                    };
                    view.preferences.theme = Some(theme.name.clone());
                    view.preferences.theme_file = Some(path);
                    view.theme_family = Some(family);
                    view.appearance_mode = None;
                    view.save_preferences();
                    publish_preferences(cx, &view.preferences);
                    cx.notify();
                }
                Err(error) => view.report_error(format!("Theme import failed: {error}")),
            });
        })
        .detach();
    }

    fn update_appearance(&mut self, window: &Window) {
        let selected_theme = if self.preferences.theme.is_none() {
            self.zed_theme.as_ref()
        } else {
            self.preferences.theme.as_deref().and_then(|name| {
                self.theme_family
                    .as_ref()
                    .and_then(|family| family.member(name))
            })
        };
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
        if self.appearance_mode == Some(mode) {
            return;
        }
        set_window_appearance(
            window,
            (self.preferences.theme.is_some() || self.zed_theme.is_some())
                .then_some(mode == AppearanceMode::Dark),
        );
        let Some(generation) = self.bridge_context.generation else {
            return;
        };
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
            web_view.set_fonts(self.zed_fonts.as_ref().unwrap_or(&ZedFonts::default()));
        }
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
        let messages = self
            .web_view
            .as_ref()
            .map_or_else(Vec::new, EmbeddedWebView::drain_bridge_messages);
        for message in messages {
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
                            publish_preferences(cx, &self.preferences);
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
                                crate::contracts::ThemeAction::Import => self.import_theme(cx),
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
                                publish_preferences(cx, &self.preferences);
                            }
                            cx.notify();
                        }
                        match action.action {
                            ActionMessage::Open(crate::contracts::OpenAction::File) => {
                                cx.defer(prompt_for_document);
                            }
                            ActionMessage::Open(crate::contracts::OpenAction::Folder) => {
                                cx.defer(prompt_for_directory);
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
        let Some(mut web_view) = EmbeddedWebView::attach(window, MAIN_TITLEBAR_HEIGHT) else {
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
        cx.add_recent_document(&current.path);
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
        self.shell.current_document = Some(path.clone());
        self.preferences.record_recent(path);
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

    fn titlebar(
        &self,
        active: bool,
        palette: UiPalette,
        title: &str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if !active {
            return div().h(px(0.0)).into_any_element();
        }
        let drag = |id| {
            div()
                .id(id)
                .h_full()
                .on_mouse_down(MouseButton::Left, |_, window, _| begin_window_drag(window))
        };
        let button = |id, icon: &'static str, tooltip: &'static str| {
            div()
                .id(id)
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .hover(move |style| style.bg(palette.control))
                .tooltip(move |_, cx| {
                    cx.new(|_| TextTooltip {
                        text: tooltip,
                        palette,
                    })
                    .into()
                })
                .child(icon)
        };
        let mut bar = div()
            .h(px(MAIN_TITLEBAR_HEIGHT as f32))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .bg(palette.background)
            .text_color(palette.foreground)
            .font_family(
                self.zed_fonts
                    .as_ref()
                    .and_then(|fonts| fonts.ui_family.clone())
                    .unwrap_or_else(|| "SF Mono".into()),
            )
            .text_size(px(self
                .zed_fonts
                .as_ref()
                .and_then(|fonts| fonts.ui_size)
                .unwrap_or(12.0)))
            .when_some(
                self.zed_fonts
                    .as_ref()
                    .and_then(|fonts| fonts.ui_weight.as_deref())
                    .and_then(ui_font_weight),
                |bar, weight| bar.font_weight(weight),
            )
            .border_b_1()
            .border_color(palette.control)
            .child(drag("titlebar-drag-left").w(px(72.0)));

        if self.web_view.is_some() {
            bar = bar
                .child(
                    button("titlebar-back", "‹", "Back")
                        .when(!self.navigation.can_go_back(), |button| {
                            button.opacity(0.35)
                        })
                        .on_click(cx.listener(|view, _, _, cx| view.go_back(cx))),
                )
                .child(
                    button("titlebar-forward", "›", "Forward")
                        .when(!self.navigation.can_go_forward(), |button| {
                            button.opacity(0.35)
                        })
                        .on_click(cx.listener(|view, _, _, cx| view.go_forward(cx))),
                )
                .child(
                    button("titlebar-open-file", "▱", "Open File")
                        .on_click(|_, _, cx| cx.defer(prompt_for_document)),
                )
                .child(
                    button("titlebar-open-folder", "▰", "Open Folder")
                        .on_click(|_, _, cx| cx.defer(prompt_for_directory)),
                )
                .child(
                    button("titlebar-copy-markdown", "⧉", "Copy Markdown").on_click(cx.listener(
                        |view, _, _, _| {
                            if let Some(web_view) = view.web_view.as_ref() {
                                web_view.copy_source();
                            }
                        },
                    )),
                )
                .child(
                    button("titlebar-copy-rendered", "◈", "Copy Rendered").on_click(cx.listener(
                        |view, _, _, _| {
                            if let Some(web_view) = view.web_view.as_ref() {
                                web_view.copy_rendered();
                            }
                        },
                    )),
                );
        }

        bar.child(
            drag("titlebar-drag-center")
                .flex_1()
                .min_w(px(32.0))
                .flex()
                .items_center()
                .justify_end()
                .pr_3()
                .text_xs()
                .child(title.to_owned()),
        )
        .into_any_element()
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
        let _ = self.appearance_task.take();
        let _ = &self.bridge_task;
    }
}

impl Render for MdvrView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self
            .shell
            .current_document
            .as_deref()
            .and_then(std::path::Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("mdvr")
            .to_owned();
        window.set_window_title(&title);
        let active = window.is_window_active();
        let maximized = window.is_maximized();
        set_window_background_draggable(window, active && self.web_view.is_some());
        set_titlebar_controls_visible(window, active || maximized);
        self.process_next_open(window, cx);
        if self.initialized {
            self.update_appearance(window);
            self.capture_window_geometry(window);
        }
        let shared = cx.global::<SharedPreferences>().clone();
        let palette = ui_palette(&self.preferences, &shared, window);
        let titlebar = self.titlebar(active || maximized, palette, &title, cx);
        if self.loading_document {
            return main_window_frame(
                titlebar,
                div().size_full().bg(palette.background).into_any_element(),
            );
        }
        if let Some(web_view) = self.web_view.as_ref() {
            web_view.sync_frame(if active || maximized {
                MAIN_TITLEBAR_HEIGHT
            } else {
                0.0
            });
            return main_window_frame(titlebar, div().size_full().into_any_element());
        }
        if let Some((path, bytes)) = self.pending_large.as_ref() {
            let content = div()
                .size_full()
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .gap_3()
                .bg(palette.background)
                .text_color(palette.foreground)
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
                        .bg(palette.control)
                        .child("Open full file")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.confirm_large_file(window, cx);
                        })),
                )
                .into_any_element();
            return main_window_frame(titlebar, content);
        }
        window.focus(&self.picker_focus);
        let selected = self.shell.picker.selected().map(str::to_owned);
        let entries = self.shell.picker.visible();
        let document_count = entries.len();
        let root = self.shell.root.clone().unwrap_or_default();
        let can_retry = self.failed_path.is_some();
        let content = div()
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
            .bg(palette.background)
            .text_color(palette.foreground)
            .child(
                div()
                    .id("picker-filter")
                    .w_full()
                    .px_3()
                    .py_2()
                    .rounded_sm()
                    .border_1()
                    .border_color(palette.accent)
                    .bg(palette.control)
                    .child(if self.shell.picker.query().is_empty() {
                        "Filter files…".to_owned()
                    } else {
                        self.shell.picker.query().to_owned()
                    }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(palette.foreground)
                    .opacity(0.65)
                    .child(format!(
                        "{document_count} document{}",
                        if document_count == 1 { "" } else { "s" }
                    )),
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
                                    .bg(palette.control)
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
                                .bg(palette.control)
                                .child("Choose file")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.prompt_for_picker_document(cx);
                                })),
                        )
                        .child(
                            div()
                                .id("browse-folder-after-error")
                                .px_3()
                                .py_2()
                                .rounded_sm()
                                .cursor_pointer()
                                .bg(palette.control)
                                .child("Browse folder")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.prompt_for_picker_directory(cx);
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
                        let modified = relative_mtime(&root, &path);
                        div()
                            .id(("picker-entry", index))
                            .px_3()
                            .py_2()
                            .border_l_2()
                            .border_color(if is_selected {
                                palette.accent
                            } else {
                                palette.background
                            })
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(move |style| style.bg(palette.control))
                            .when(is_selected, |style| style.bg(palette.selection))
                            .text_color(palette.foreground)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(path.clone())
                                    .child(div().text_sm().opacity(0.65).child(modified)),
                            )
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
                                    .bg(palette.control)
                                    .child("Choose folder")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.prompt_for_picker_directory(cx);
                                    })),
                            ),
                    )
                },
            )
            .into_any_element();
        main_window_frame(titlebar, content)
    }
}

struct PreferencesView {
    preferences: Preferences,
    web_view: EmbeddedWebView,
    themes: Vec<Option<String>>,
    #[allow(dead_code)]
    preferences_subscription: Subscription,
    #[allow(dead_code)]
    poll_task: Option<Task<()>>,
}

impl PreferencesView {
    fn poll(&mut self, cx: &mut Context<Self>) {
        for message in self.web_view.drain_preferences() {
            self.preferences.use_zed_config = message.use_zed;
            self.preferences.theme = message.theme;
            let _ = self.preferences.set_text_scale(message.scale);
            publish_preferences(cx, &self.preferences);
        }
        self.web_view.flush_pending();
        cx.notify();
    }
}

impl Render for PreferencesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title("Preferences");
        let shared = cx.global::<SharedPreferences>();
        let palette = ui_palette(&self.preferences, shared, window);
        set_window_appearance(
            window,
            (self.preferences.theme.is_some() || shared.zed_theme.is_some())
                .then_some(palette.dark),
        );
        self.web_view.set_preferences(
            &PreferencesMessage {
                use_zed: self.preferences.use_zed_config,
                theme: self.preferences.theme.clone(),
                scale: self.preferences.text_scale_percent,
            },
            &self.themes,
            palette.dark,
            &format!(
                "#{:02x}{:02x}{:02x}",
                (palette.background.r * 255.0) as u8,
                (palette.background.g * 255.0) as u8,
                (palette.background.b * 255.0) as u8
            ),
            &format!(
                "#{:02x}{:02x}{:02x}",
                (palette.foreground.r * 255.0) as u8,
                (palette.foreground.g * 255.0) as u8,
                (palette.foreground.b * 255.0) as u8
            ),
            &format!(
                "#{:02x}{:02x}{:02x}",
                (palette.control.r * 255.0) as u8,
                (palette.control.g * 255.0) as u8,
                (palette.control.b * 255.0) as u8
            ),
        );
        self.web_view.sync_frame(0.0);
        div().size_full()
    }
}

fn active_mdvr_window(cx: &mut App) -> Option<gpui::WindowHandle<MdvrView>> {
    cx.active_window()
        .and_then(|window| window.downcast::<MdvrView>())
}

fn prompt_for_path(
    cx: &mut App,
    options: PathPromptOptions,
    on_selected: impl FnOnce(PathBuf, &mut App) + 'static,
) {
    let receiver = cx.prompt_for_paths(options);
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = receiver.await else {
            return;
        };
        let Some(path) = paths.into_iter().next() else {
            return;
        };
        let _ = cx.update(|cx| on_selected(path, cx));
    })
    .detach();
}

fn prompt_for_document(cx: &mut App) {
    prompt_for_path(
        cx,
        PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open Markdown".into()),
        },
        |path, cx| {
            let markdown = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown")
                });
            if markdown {
                open_new_window(cx, path, LaunchIntent::ExplicitFile);
            } else {
                eprintln!("mdvr: selected file is not Markdown: {}", path.display());
            }
        },
    );
}

fn prompt_for_directory(cx: &mut App) {
    prompt_for_path(
        cx,
        PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Folder".into()),
        },
        |path, cx| open_new_window(cx, path, LaunchIntent::ExplicitDirectory),
    );
}

fn open_new_window(cx: &mut App, path: PathBuf, intent: LaunchIntent) {
    let preferences = conventional_path()
        .map(|path| load_or_default(&path).preferences)
        .unwrap_or_default();
    let state = match resolve_launch(intent, Some(path.as_path()), &preferences, false) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("mdvr: cannot open {}: {error}", path.display());
            return;
        }
    };
    let picker_root = state
        .browsing_root
        .clone()
        .unwrap_or_else(|| path.parent().unwrap_or(path.as_path()).to_owned());
    open_mdvr_window(
        cx,
        LaunchPlan {
            intent,
            state,
            picker_root,
            explicit: true,
        },
        true,
    );
}

fn open_file_action(_: &OpenFileAction, cx: &mut App) {
    cx.defer(prompt_for_document);
}

fn open_recent_action(action: &OpenRecent, cx: &mut App) {
    open_new_window(cx, PathBuf::from(&action.path), LaunchIntent::ExplicitFile);
}

fn close_window_action(_: &CloseWindow, cx: &mut App) {
    if let Some(window) = cx.active_window() {
        let _ = window.update(cx, |_, window, _| window.remove_window());
    }
}

fn show_picker_action(_: &ShowPicker, cx: &mut App) {
    if let Some(window) = active_mdvr_window(cx) {
        let _ = window.update(cx, |view, _, cx| view.open_document_picker(cx));
    }
}

fn open_folder_action(_: &OpenFolderAction, cx: &mut App) {
    cx.defer(prompt_for_directory);
}

fn show_preferences(_: &ShowPreferences, cx: &mut App) {
    if cx
        .windows()
        .into_iter()
        .any(|window| window.downcast::<PreferencesView>().is_some())
    {
        return;
    }
    let shared = cx.global::<SharedPreferences>().clone();
    let preferences = shared.preferences.clone();
    let mut themes = vec![None, Some("light".into()), Some("dark".into())];
    for theme in &shared.theme_family.members {
        let name = Some(theme.name.clone());
        if !themes.contains(&name) {
            themes.push(name);
        }
    }
    let _ = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(300.0), px(200.0)),
                size: size(px(430.0), px(250.0)),
            })),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("Preferences".into()),
                appears_transparent: false,
                traffic_light_position: None,
            }),
            is_resizable: false,
            ..WindowOptions::default()
        },
        move |window, cx| {
            let mut web_view =
                EmbeddedWebView::attach_preferences(window).expect("preferences web view");
            web_view.load_initial_document();
            let view = cx.new(|cx| {
                let preferences_subscription =
                    cx.observe_global::<SharedPreferences>(|view: &mut PreferencesView, cx| {
                        view.preferences = cx.global::<SharedPreferences>().preferences.clone();
                        cx.notify();
                    });
                PreferencesView {
                    preferences,
                    web_view,
                    themes,
                    preferences_subscription,
                    poll_task: None,
                }
            });
            let poll_view = view.downgrade();
            let poll_task = cx.spawn(async move |cx| {
                loop {
                    Timer::after(Duration::from_millis(100)).await;
                    if poll_view.update(cx, |view, cx| view.poll(cx)).is_err() {
                        return;
                    }
                }
            });
            view.update(cx, |view, _| view.poll_task = Some(poll_task));
            view
        },
    );
}

fn edit_menu_action<T>(_: &T, _: &mut App) {}

fn undo_action(_: &Undo, _: &mut App) {
    perform_undo();
}

fn redo_action(_: &Redo, _: &mut App) {
    perform_redo();
}

fn quit_app(_: &QuitApp, cx: &mut App) {
    cx.quit();
}

fn about_mdvr(_: &AboutMdvr, _cx: &mut App) {
    show_about();
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
    let parent_origin = cx.active_window().and_then(|window| {
        window
            .update(cx, |_, window, _| window.bounds().origin)
            .ok()
    });
    let mut geometry = conventional_path()
        .map(|path| load_or_default(&path).preferences.window)
        .unwrap_or_default();
    if let Some(origin) = parent_origin {
        geometry.x = (origin.x / px(1.0)).round() as i32 + 24;
        geometry.y = (origin.y / px(1.0)).round() as i32 + 24;
    }
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
        install_close_shortcut();
        let preferences = conventional_path()
            .map(|path| load_or_default(&path).preferences)
            .unwrap_or_default();
        cx.set_global(SharedPreferences::new(preferences.clone()));
        cx.on_action(open_file_action);
        cx.on_action(open_recent_action);
        cx.on_action(open_folder_action);
        cx.on_action(show_picker_action);
        cx.on_action(close_window_action);
        cx.on_action(edit_menu_action::<Cut>);
        cx.on_action(edit_menu_action::<Copy>);
        cx.on_action(edit_menu_action::<Paste>);
        cx.on_action(edit_menu_action::<SelectAll>);
        cx.on_action(undo_action);
        cx.on_action(redo_action);
        cx.on_action(show_preferences);
        cx.on_action(quit_app);
        cx.on_action(about_mdvr);
        cx.bind_keys([
            gpui::KeyBinding::new("cmd-o", OpenFileAction, None),
            gpui::KeyBinding::new("cmd-shift-o", OpenFolderAction, None),
            gpui::KeyBinding::new("cmd-p", ShowPicker, None),
            gpui::KeyBinding::new("cmd-w", CloseWindow, None),
            gpui::KeyBinding::new("cmd-x", Cut, None),
            gpui::KeyBinding::new("cmd-c", Copy, None),
            gpui::KeyBinding::new("cmd-v", Paste, None),
            gpui::KeyBinding::new("cmd-a", SelectAll, None),
            gpui::KeyBinding::new("cmd-z", Undo, None),
            gpui::KeyBinding::new("cmd-shift-z", Redo, None),
            gpui::KeyBinding::new("cmd-,", ShowPreferences, None),
            gpui::KeyBinding::new("cmd-q", QuitApp, None),
        ]);
        let recent_items = preferences
            .recent_files
            .iter()
            .filter_map(|path| {
                Some(MenuItem::action(
                    path.file_name()?.to_string_lossy().to_string(),
                    OpenRecent {
                        path: path.to_string_lossy().to_string(),
                    },
                ))
            })
            .collect();
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
                    MenuItem::submenu(Menu {
                        name: "Open Recent".into(),
                        items: recent_items,
                    }),
                    MenuItem::separator(),
                    MenuItem::action("Browse Files", ShowPicker),
                    MenuItem::action("Close Window", CloseWindow),
                ],
            },
            Menu {
                name: "Edit".into(),
                items: vec![
                    MenuItem::os_action("Undo", Undo, OsAction::Undo),
                    MenuItem::os_action("Redo", Redo, OsAction::Redo),
                    MenuItem::separator(),
                    MenuItem::os_action("Cut", Cut, OsAction::Cut),
                    MenuItem::os_action("Copy", Copy, OsAction::Copy),
                    MenuItem::os_action("Paste", Paste, OsAction::Paste),
                    MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
                ],
            },
            Menu {
                name: "Window".into(),
                items: vec![MenuItem::action("Close Window", CloseWindow)],
            },
            Menu {
                name: "Help".into(),
                items: vec![MenuItem::action("About mdvr", AboutMdvr)],
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
