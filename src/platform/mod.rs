#![allow(unexpected_cfgs)]

#[allow(dead_code)]
pub(crate) mod bridge;
#[allow(dead_code)]
pub(crate) mod remote_fetch;
#[allow(dead_code)]
pub(crate) mod remote_policy;
#[allow(dead_code)]
pub(crate) mod resource_policy;

use std::{
    ffi::CStr,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use crate::{
    contracts::{
        Appearance, AppearanceUpdate, ContractError, DocumentId, Envelope, Generation, Locator,
        MAX_FRAME_BYTES, Message, ResourceResult, decode, encode,
    },
    platform::bridge::{BridgeContext, BridgeMessage, BridgeRouter},
};
use cocoa::{
    appkit::NSView,
    base::{BOOL, id, nil},
    foundation::NSString,
};
use gpui::Window;
use objc::{class, msg_send, sel, sel_impl};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use wry::{
    PageLoadEvent, Rect, WebView, WebViewBuilder,
    http::{Request, Response, header::CONTENT_TYPE},
};

static ACCEPTED_BRIDGE_MESSAGES: AtomicU64 = AtomicU64::new(0);
static REJECTED_BRIDGE_MESSAGES: AtomicU64 = AtomicU64::new(0);
static BRIDGE_ROUTER: OnceLock<Mutex<BridgeRouter>> = OnceLock::new();

const DEV_WEB_ASSET_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/web/dist");

fn valid_web_asset_root(root: &Path) -> Option<PathBuf> {
    let metadata = fs::symlink_metadata(root).ok()?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return None;
    }

    let root = root.canonicalize().ok()?;
    let entry = root.join("index.html");
    let entry_metadata = fs::symlink_metadata(&entry).ok()?;
    if !entry_metadata.file_type().is_file() || entry_metadata.file_type().is_symlink() {
        return None;
    }
    if entry.canonicalize().ok()?.parent() != Some(root.as_path()) {
        return None;
    }
    Some(root)
}

fn resolve_web_asset_root(executable: &Path, development_root: &Path) -> Option<PathBuf> {
    let packaged_root = executable.parent()?.parent()?.join("Resources").join("web");
    [packaged_root, development_root.to_path_buf()]
        .into_iter()
        .find_map(|root| valid_web_asset_root(&root))
}

fn navigation_allowed_url(url: String) -> bool {
    url == "mdvr://localhost/index.html"
}

fn asset_path(root: &Path, relative: &str) -> Option<PathBuf> {
    let relative = relative.trim_start_matches('/');
    let path = root.join(if relative.is_empty() {
        "index.html"
    } else {
        relative
    });
    path.canonicalize()
        .ok()
        .filter(|path| path.starts_with(root))
}

fn content_type(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn canonical_bridge_message(bytes: &[u8]) -> Result<Vec<u8>, ContractError> {
    encode(&decode(bytes)?)
}

fn main_thread() -> bool {
    unsafe {
        let is_main: objc::runtime::BOOL = msg_send![class!(NSThread), isMainThread];
        is_main == objc::runtime::YES
    }
}

fn reject_bridge_message() {
    REJECTED_BRIDGE_MESSAGES.fetch_add(1, Ordering::Relaxed);
}

/// Drain validated actions without exposing paths, files, or native handles.
pub(crate) fn drain_bridge_messages() -> Vec<BridgeMessage> {
    BRIDGE_ROUTER
        .get()
        .and_then(|router| router.lock().ok())
        .map_or_else(Vec::new, |mut router| router.drain())
}

/// Update action authorization context after native document commit.
fn confirm(title: &str, detail: &str, allow: &str, cancel: &str) -> bool {
    if !main_thread() {
        return false;
    }
    unsafe {
        let alert: id = msg_send![class!(NSAlert), new];
        if alert.is_null() {
            return false;
        }
        let title = NSString::alloc(nil).init_str(title);
        let detail = NSString::alloc(nil).init_str(detail);
        let allow = NSString::alloc(nil).init_str(allow);
        let cancel = NSString::alloc(nil).init_str(cancel);
        let _: () = msg_send![alert, setMessageText: title];
        let _: () = msg_send![alert, setInformativeText: detail];
        let _: id = msg_send![alert, addButtonWithTitle: allow];
        let _: id = msg_send![alert, addButtonWithTitle: cancel];
        let application: id = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![application, activateIgnoringOtherApps: true];
        let window: id = msg_send![alert, window];
        let _: () = msg_send![window, makeKeyAndOrderFront: nil];
        let response: isize = msg_send![alert, runModal];
        let _: () = msg_send![title, release];
        let _: () = msg_send![detail, release];
        let _: () = msg_send![allow, release];
        let _: () = msg_send![cancel, release];
        let _: () = msg_send![alert, release];
        response == 1000
    }
}

pub(crate) fn confirm_large_document(path: &Path, bytes: usize) -> bool {
    confirm(
        "Open this large Markdown file?",
        &format!("{} ({:.1} MiB)", path.display(), bytes as f64 / 1_048_576.0),
        "Open Full File",
        "Cancel",
    )
}

pub(crate) fn confirm_remote_images(url: &str) -> bool {
    confirm(
        "Load remote images for this document?",
        url,
        "Load remote images",
        "Keep blocked",
    )
}

pub(crate) fn confirm_outside_resource(path: &Path) -> bool {
    confirm(
        "Allow image outside document folder?",
        &path.display().to_string(),
        "Allow this image",
        "Cancel",
    )
}

fn canonical_safe_local_file(path: &Path) -> Option<PathBuf> {
    let path = path.canonicalize().ok()?;
    let metadata = path.metadata().ok()?;
    (metadata.is_file() && metadata.permissions().mode() & 0o111 == 0).then_some(path)
}

pub(crate) fn open_local_file(path: &Path, require_confirmation: bool) -> bool {
    if !main_thread() {
        return false;
    }
    let Some(path) = canonical_safe_local_file(path) else {
        return false;
    };
    if require_confirmation
        && !confirm(
            "Open this local file in its default app?",
            &path.display().to_string(),
            "Open File",
            "Cancel",
        )
    {
        return false;
    }
    unsafe {
        let value = NSString::alloc(nil).init_str(&path.display().to_string());
        let target: id = msg_send![class!(NSURL), fileURLWithPath: value];
        let _: () = msg_send![value, release];
        if target.is_null() {
            return false;
        }
        let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let opened: BOOL = msg_send![workspace, openURL: target];
        opened == objc::runtime::YES
    }
}

fn choose_path(files: bool, directories: bool, extensions: &[&str]) -> Option<PathBuf> {
    if !main_thread() {
        return None;
    }
    unsafe {
        let panel: id = msg_send![class!(NSOpenPanel), openPanel];
        if panel.is_null() {
            eprintln!("mdvr: NSOpenPanel unavailable");
            return None;
        }
        let _: () = msg_send![panel, setCanChooseFiles: files];
        let _: () = msg_send![panel, setCanChooseDirectories: directories];
        let _: () = msg_send![panel, setAllowsMultipleSelection: false];
        if !extensions.is_empty() {
            let types: id = msg_send![class!(NSMutableArray), array];
            for extension in extensions {
                let value = NSString::alloc(nil).init_str(extension);
                let _: () = msg_send![types, addObject: value];
                let _: () = msg_send![value, release];
            }
            let _: () = msg_send![panel, setAllowedFileTypes: types];
        }
        let response: isize = msg_send![panel, runModal];
        if response != 1 {
            return None;
        }
        let url: id = msg_send![panel, URL];
        let path: id = msg_send![url, path];
        let bytes: *const std::os::raw::c_char = msg_send![path, UTF8String];
        (!bytes.is_null()).then(|| PathBuf::from(CStr::from_ptr(bytes).to_string_lossy().as_ref()))
    }
}

pub(crate) fn choose_json_file() -> Option<PathBuf> {
    choose_path(true, false, &["json"])
}

pub(crate) fn choose_markdown_file() -> Option<PathBuf> {
    choose_path(true, false, &["md", "markdown"])
}

pub(crate) fn choose_directory() -> Option<PathBuf> {
    choose_path(false, true, &[])
}

pub(crate) fn file_url_path(url: &str) -> Option<PathBuf> {
    if url.contains(char::is_control) {
        return None;
    }
    reqwest::Url::parse(url).ok()?.to_file_path().ok()
}

pub(crate) fn open_external_url(url: &str) -> bool {
    if !main_thread() || url.contains(char::is_control) {
        return false;
    }
    unsafe {
        let value = NSString::alloc(nil).init_str(url);
        let target: id = msg_send![class!(NSURL), URLWithString: value];
        let _: () = msg_send![value, release];
        if target.is_null() {
            return false;
        }
        let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let opened: BOOL = msg_send![workspace, openURL: target];
        opened == objc::runtime::YES
    }
}

pub(crate) fn reset_bridge_messages() {
    if let Some(router) = BRIDGE_ROUTER.get()
        && let Ok(mut router) = router.lock()
    {
        router.clear();
    }
}

pub(crate) fn update_bridge_context(context: BridgeContext) {
    let router = BRIDGE_ROUTER.get_or_init(|| Mutex::new(BridgeRouter::new(context)));
    if let Ok(mut router) = router.lock() {
        router.set_context(context);
    }
}

fn receive_bridge_message(message: &str) {
    let accepted = (message.len() <= MAX_FRAME_BYTES)
        .then_some(message.as_bytes())
        .and_then(|bytes| canonical_bridge_message(bytes).ok())
        .is_some_and(|bytes| {
            BRIDGE_ROUTER
                .get_or_init(|| Mutex::new(BridgeRouter::new(BridgeContext::default())))
                .lock()
                .ok()
                .is_some_and(|mut router| router.accept(&bytes).is_ok())
        });
    if accepted {
        ACCEPTED_BRIDGE_MESSAGES.fetch_add(1, Ordering::Relaxed);
    } else {
        reject_bridge_message();
    }
}

fn evaluate_javascript(view: &WebView, script: &str) {
    if let Err(error) = view.evaluate_script(script) {
        eprintln!("mdvr: JavaScript evaluation failed: {error}");
    }
}

#[derive(Clone, Debug, PartialEq)]
struct PendingSource {
    source: String,
    generation: Generation,
}

#[derive(Clone, Debug, PartialEq)]
struct PendingAppearance {
    appearance: Appearance,
    generation: Generation,
}

#[derive(Clone, Debug, PartialEq)]
struct PendingContext {
    context: BridgeContext,
}

#[derive(Clone, Debug, PartialEq)]
struct PendingLocator {
    locator: Locator,
    generation: Generation,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq)]
struct PendingPage {
    source: Option<PendingSource>,
    appearance: Option<PendingAppearance>,
    context: Option<PendingContext>,
    theme_choices: Option<String>,
    history: Option<String>,
    locator: Option<PendingLocator>,
}

#[derive(Default)]
struct PendingPageState {
    page_ready: bool,
    latest_source_generation: Option<Generation>,
    latest_context_generation: Option<Generation>,
    source: Option<PendingSource>,
    appearance: Option<PendingAppearance>,
    context: Option<PendingContext>,
    theme_choices: Option<String>,
    history: Option<String>,
    locator: Option<PendingLocator>,
}

impl PendingPageState {
    fn begin_load(&mut self) {
        self.page_ready = false;
        self.latest_source_generation = None;
        self.latest_context_generation = None;
        self.source = None;
        self.appearance = None;
        self.context = None;
        self.theme_choices = None;
        self.history = None;
        self.locator = None;
    }

    fn page_ready(&self) -> bool {
        self.page_ready
    }

    fn advance_source_generation(&mut self, generation: Generation) {
        self.latest_source_generation = Some(generation);
        if self.context.as_ref().is_some_and(|pending| {
            pending
                .context
                .generation
                .is_some_and(|current| current.get() < generation.get())
        }) {
            self.context = None;
        }
        if self
            .appearance
            .as_ref()
            .is_some_and(|pending| pending.generation.get() < generation.get())
        {
            self.appearance = None;
        }
        if self
            .locator
            .as_ref()
            .is_some_and(|pending| pending.generation.get() < generation.get())
        {
            self.locator = None;
        }
    }

    fn replace_source(&mut self, source: String, generation: Generation) -> bool {
        if self
            .latest_source_generation
            .is_some_and(|current| generation.get() <= current.get())
        {
            return false;
        }
        self.advance_source_generation(generation);
        self.source = Some(PendingSource { source, generation });
        true
    }

    fn replace_context(&mut self, context: BridgeContext) -> bool {
        if context.generation.is_some_and(|generation| {
            self.latest_source_generation
                .is_some_and(|source| generation.get() < source.get())
                || self
                    .latest_context_generation
                    .is_some_and(|current| generation.get() < current.get())
        }) {
            return false;
        }
        if context.generation.is_some() {
            self.latest_context_generation = context.generation;
        }
        self.context = Some(PendingContext { context });
        true
    }

    fn replace_locator(&mut self, locator: Locator, generation: Generation) -> bool {
        if self
            .latest_source_generation
            .is_some_and(|source| generation.get() < source.get())
        {
            return false;
        }
        self.locator = Some(PendingLocator {
            locator,
            generation,
        });
        true
    }

    fn replace_appearance(&mut self, appearance: Appearance, generation: Generation) -> bool {
        if self
            .latest_source_generation
            .is_some_and(|source| generation.get() < source.get())
            || self
                .appearance
                .as_ref()
                .is_some_and(|pending| generation.get() < pending.generation.get())
        {
            return false;
        }
        self.appearance = Some(PendingAppearance {
            appearance,
            generation,
        });
        true
    }

    #[allow(dead_code)]
    fn take_pending(&mut self) -> PendingPage {
        self.page_ready = true;
        PendingPage {
            source: self.source.take(),
            appearance: self.appearance.take(),
            context: self.context.take(),
            theme_choices: self.theme_choices.take(),
            history: self.history.take(),
            locator: self.locator.take(),
        }
    }

    #[allow(dead_code)]
    fn apply_pending(&mut self, web_view: &WebView) {
        let pending = self.take_pending();
        if let Some(context) = pending.context {
            evaluate_javascript(
                web_view,
                &navigation_context_script(context.context.document, context.context.generation),
            );
        }
        if let Some(script) = pending.theme_choices {
            evaluate_javascript(web_view, &script);
        }
        if let Some(script) = pending.history {
            evaluate_javascript(web_view, &script);
        }
        if let Some(appearance) = pending.appearance {
            match appearance_script(&appearance.appearance) {
                Ok(script) => evaluate_javascript(web_view, &script),
                Err(error) => eprintln!("mdvr: cannot apply pending appearance: {error}"),
            }
        }
        if let Some(source) = pending.source {
            match document_load_script(&source.source, source.generation) {
                Ok(script) => evaluate_javascript(web_view, &script),
                Err(error) => eprintln!("mdvr: cannot apply pending source: {error}"),
            }
        }
        if let Some(locator) = pending.locator {
            match locator_script(&locator.locator) {
                Ok(script) => evaluate_javascript(web_view, &script),
                Err(error) => eprintln!("mdvr: cannot apply pending locator: {error}"),
            }
        }
    }
}

pub struct EmbeddedWebView {
    parent: id,
    view: WebView,
    current_generation: DocumentGeneration,
    #[allow(dead_code)]
    appearance_generation: AppearanceGeneration,
    pending_state: Box<PendingPageState>,
    page_loaded: Arc<AtomicBool>,
}

#[derive(Default)]
struct DocumentGeneration {
    current: Option<Generation>,
}

impl DocumentGeneration {
    fn accept(&mut self, generation: Generation) -> bool {
        if self
            .current
            .is_some_and(|current| generation.get() <= current.get())
        {
            return false;
        }
        self.current = Some(generation);
        true
    }

    #[allow(dead_code)]
    fn is_current(&self, generation: Generation) -> bool {
        self.current == Some(generation)
    }
}

#[derive(Default)]
#[allow(dead_code)]
struct AppearanceGeneration {
    current: Option<Generation>,
}

#[allow(dead_code)]
impl AppearanceGeneration {
    fn accept(&mut self, generation: Generation, document: &DocumentGeneration) -> bool {
        if !document.is_current(generation)
            || self
                .current
                .is_some_and(|current| generation.get() < current.get())
        {
            return false;
        }
        self.current = Some(generation);
        true
    }
}

fn document_load_script(source: &str, generation: Generation) -> Result<String, serde_json::Error> {
    Ok(format!(
        "window.mdvrLoadDocument({}, {});",
        serde_json::to_string(source)?,
        generation.get()
    ))
}

fn navigation_context_script(
    document: Option<DocumentId>,
    generation: Option<Generation>,
) -> String {
    format!(
        "window.mdvrSetNavigationContext({}, {});",
        document.map_or_else(|| "null".into(), |value| value.get().to_string()),
        generation.map_or_else(|| "null".into(), |value| value.get().to_string())
    )
}

fn anchor_script(anchor: &str) -> Result<String, serde_json::Error> {
    Ok(format!(
        "window.mdvrNavigateAnchor({});",
        serde_json::to_string(anchor)?
    ))
}

fn error_script(message: &str) -> Result<String, serde_json::Error> {
    Ok(format!(
        "window.mdvrShowError({});",
        serde_json::to_string(message)?
    ))
}

fn locator_script(locator: &Locator) -> Result<String, serde_json::Error> {
    Ok(format!(
        "window.mdvrRestoreLocator({});",
        serde_json::to_string(locator)?
    ))
}

#[allow(dead_code)]
fn appearance_script(appearance: &Appearance) -> Result<String, ContractError> {
    let envelope = Envelope::new(Message::AppearanceUpdate(AppearanceUpdate {
        document: None,
        appearance: appearance.clone(),
    }));
    encode(&envelope)?;
    let tokens = serde_json::to_string(appearance)
        .map_err(|error| ContractError::Json(error.to_string()))?;
    Ok(format!("window.mdvrApplyAppearance({tokens});"))
}

impl EmbeddedWebView {
    pub fn focus(&self) -> bool {
        self.view.focus().is_ok()
    }

    pub fn sync_frame(&self) {
        assert!(main_thread(), "Wry WebView must be used on main thread");
        unsafe {
            let bounds = NSView::bounds(self.parent);
            let _ = self.view.set_bounds(Rect {
                position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
                size: wry::dpi::LogicalSize::new(bounds.size.width, bounds.size.height).into(),
            });
        }
    }

    pub fn attach(window: &Window) -> Option<Self> {
        if !main_thread() {
            return None;
        }
        let executable = std::env::current_exe().ok()?;
        let development_root = Path::new(DEV_WEB_ASSET_ROOT);
        let root = resolve_web_asset_root(&executable, development_root)?;
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let parent = match handle.as_raw() {
            RawWindowHandle::AppKit(handle) => handle.ns_view.as_ptr() as id,
            _ => return None,
        };
        let initial_bounds = unsafe { NSView::bounds(parent) };
        let page_loaded = Arc::new(AtomicBool::new(false));
        let page_loaded_callback = page_loaded.clone();
        let protocol_root = root.clone();
        let view = WebViewBuilder::new()
            .with_custom_protocol("mdvr".into(), move |_id, request: Request<Vec<u8>>| {
                let relative = request.uri().path().trim_start_matches('/');
                let response =
                    asset_path(&protocol_root, relative).and_then(|path| fs::read(path).ok());
                match response {
                    Some(body) => Response::builder()
                        .header(CONTENT_TYPE, content_type(relative))
                        .body(body)
                        .expect("valid Wry asset response")
                        .map(Into::into),
                    None => Response::builder()
                        .status(404)
                        .body(Vec::new())
                        .expect("valid Wry error response")
                        .map(Into::into),
                }
            })
            .with_ipc_handler(|request| receive_bridge_message(request.body()))
            .with_on_page_load_handler(move |event, url| {
                if matches!(event, PageLoadEvent::Finished) && url == "mdvr://localhost/index.html"
                {
                    page_loaded_callback.store(true, Ordering::Release);
                }
            })
            .with_incognito(true)
            .with_navigation_handler(navigation_allowed_url)
            .with_url("mdvr://localhost/index.html")
            .with_bounds(Rect {
                position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
                size: wry::dpi::LogicalSize::new(
                    initial_bounds.size.width,
                    initial_bounds.size.height,
                )
                .into(),
            })
            .build_as_child(window)
            .ok()?;
        Some(Self {
            parent,
            view,
            current_generation: DocumentGeneration::default(),
            appearance_generation: AppearanceGeneration::default(),
            pending_state: Box::new(PendingPageState::default()),
            page_loaded,
        })
    }

    /// Load production web assets from the packaged app or development checkout.
    /// Wry may read only this canonical app-owned asset directory.
    pub fn load_initial_document(&mut self) -> bool {
        assert!(main_thread(), "Wry WebView must be used on main thread");
        self.pending_state.begin_load();
        // Wry owns navigation and asset loading; its custom protocol started in attach.
        true
    }

    /// Apply state queued while Wry loads bundled renderer assets.
    pub fn flush_pending(&mut self) {
        if !self.pending_state.page_ready() && self.page_loaded.load(Ordering::Acquire) {
            self.pending_state.apply_pending(&self.view);
        }
    }

    /// Push trusted native source into the already-loaded bundled renderer.
    /// Stale generations leave the current rendered document untouched.
    pub fn load_document_source(
        &mut self,
        source: &str,
        generation: Generation,
    ) -> Result<bool, serde_json::Error> {
        assert!(main_thread(), "Wry WebView must be used on main thread");
        let script = document_load_script(source, generation)?;
        if !self.current_generation.accept(generation) {
            return Ok(false);
        }

        if self.pending_state.page_ready() {
            self.pending_state.advance_source_generation(generation);
            evaluate_javascript(&self.view, &script);
        } else {
            let accepted = self
                .pending_state
                .replace_source(source.to_owned(), generation);
            debug_assert!(accepted);
        }
        Ok(true)
    }

    pub fn set_navigation_context(
        &mut self,
        document: Option<DocumentId>,
        generation: Option<Generation>,
    ) -> Result<(), serde_json::Error> {
        assert!(main_thread(), "Wry WebView must be used on main thread");
        let context = BridgeContext {
            document,
            generation,
        };
        if !self.pending_state.replace_context(context) {
            return Ok(());
        }
        if self.pending_state.page_ready() {
            evaluate_javascript(&self.view, &navigation_context_script(document, generation));
        }
        Ok(())
    }

    pub fn navigate_anchor(&self, anchor: &str) -> Result<(), serde_json::Error> {
        assert!(main_thread(), "Wry WebView must be used on main thread");
        let script = anchor_script(anchor)?;
        if self.pending_state.page_ready() {
            evaluate_javascript(&self.view, &script);
        }
        Ok(())
    }

    pub fn clear_error(&self) {
        assert!(main_thread(), "Wry WebView must be used on main thread");
        if self.pending_state.page_ready() {
            evaluate_javascript(&self.view, "window.mdvrClearError();");
        }
    }

    pub fn show_error(&self, message: &str) -> Result<(), serde_json::Error> {
        assert!(main_thread(), "Wry WebView must be used on main thread");
        if self.pending_state.page_ready() {
            evaluate_javascript(&self.view, &error_script(message)?);
        }
        Ok(())
    }

    pub fn restore_locator(
        &mut self,
        locator: Locator,
        generation: Generation,
    ) -> Result<(), serde_json::Error> {
        assert!(main_thread(), "Wry WebView must be used on main thread");
        let script = locator_script(&locator)?;
        if self.pending_state.page_ready() {
            evaluate_javascript(&self.view, &script);
        } else {
            self.pending_state.replace_locator(locator, generation);
        }
        Ok(())
    }

    pub fn set_theme_choices(&mut self, names: &[String], selected: Option<&str>) {
        assert!(main_thread(), "Wry WebView must be used on main thread");
        let names = serde_json::to_string(names).expect("theme names serialize");
        let selected = serde_json::to_string(&selected).expect("theme selection serializes");
        let script = format!("window.mdvrSetThemeChoices({names}, {selected});");
        if self.pending_state.page_ready() {
            evaluate_javascript(&self.view, &script);
        } else {
            self.pending_state.theme_choices = Some(script);
        }
    }

    pub fn set_history_availability(&mut self, back: bool, forward: bool) {
        assert!(main_thread(), "Wry WebView must be used on main thread");
        let script = format!("window.mdvrSetHistoryAvailability({back}, {forward});");
        if self.pending_state.page_ready() {
            evaluate_javascript(&self.view, &script);
        } else {
            self.pending_state.history = Some(script);
        }
    }

    pub fn deliver_resource(&self, result: ResourceResult) -> Result<(), ContractError> {
        assert!(main_thread(), "Wry WebView must be used on main thread");
        let json = String::from_utf8(encode(&Envelope::new(Message::ResourceResult(result)))?)
            .expect("JSON encoding is UTF-8");
        evaluate_javascript(
            &self.view,
            &format!("void window.mdvrResolveResource({json});"),
        );
        Ok(())
    }

    /// Push validated native appearance tokens into the bundled renderer.
    /// Same-generation updates are allowed; older document generations are not.
    #[allow(dead_code)]
    pub fn apply_appearance(
        &mut self,
        appearance: &Appearance,
        generation: Generation,
    ) -> Result<bool, ContractError> {
        assert!(main_thread(), "Wry WebView must be used on main thread");
        let script = appearance_script(appearance)?;
        if !self
            .appearance_generation
            .accept(generation, &self.current_generation)
        {
            return Ok(false);
        }

        if !self
            .pending_state
            .replace_appearance(appearance.clone(), generation)
        {
            return Ok(false);
        }
        if self.pending_state.page_ready() {
            evaluate_javascript(&self.view, &script);
        }
        Ok(true)
    }
}

impl Drop for EmbeddedWebView {
    fn drop(&mut self) {
        assert!(
            main_thread(),
            "Wry WebView must be torn down on main thread"
        );
        reset_bridge_messages();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_decode_without_main_thread_state() {
        assert_eq!(
            file_url_path("file:///tmp/space%20document.md"),
            Some(PathBuf::from("/tmp/space document.md"))
        );
        assert_eq!(file_url_path("https://example.com/file.md"), None);
        assert_eq!(file_url_path("file:///tmp/bad\nname.md"), None);
    }

    #[test]
    fn local_file_policy_rejects_executables() {
        let path = std::env::temp_dir().join(format!("mdvr-local-file-{}", std::process::id()));
        fs::write(&path, b"safe").unwrap();
        assert_eq!(canonical_safe_local_file(&path), path.canonicalize().ok());
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        assert_eq!(canonical_safe_local_file(&path), None);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn pending_source_keeps_latest_generation_and_drains_once() {
        let mut state = PendingPageState::default();
        state.begin_load();
        let first = Generation::new(1).unwrap();
        let second = Generation::new(2).unwrap();
        assert!(state.replace_source("old".into(), first));
        assert!(state.replace_source("new".into(), second));
        assert!(!state.replace_source("stale".into(), first));
        let locator = Locator {
            heading: Some("intro".into()),
            block: "p-2".into(),
            offset: 9,
            fallback: crate::contracts::LocatorFallback::NearestHeading,
        };
        assert!(state.replace_locator(locator.clone(), second));
        state.history = Some("history".into());
        assert!(!state.page_ready());
        let pending = state.take_pending();
        assert_eq!(pending.source.unwrap().source, "new");
        assert_eq!(pending.locator.unwrap().locator, locator);
        assert_eq!(pending.history.as_deref(), Some("history"));
        assert!(state.page_ready());
        assert_eq!(state.take_pending(), PendingPage::default());
        assert!(!state.replace_source("duplicate".into(), second));
    }

    #[test]
    fn packaged_assets_precede_development_assets() {
        let base = std::env::temp_dir().join(format!(
            "mdvr-platform-assets-{}-packaged",
            std::process::id()
        ));
        let app_root = base.join("mdvr.app/Contents");
        let packaged = app_root.join("Resources/web");
        let development = base.join("checkout/web/dist");
        fs::create_dir_all(&packaged).unwrap();
        fs::create_dir_all(&development).unwrap();
        fs::write(
            packaged.join("index.html"),
            "<script type=module src=./bundle.js>",
        )
        .unwrap();
        fs::write(development.join("index.html"), "development").unwrap();

        let executable = app_root.join("MacOS/mdvr");
        assert_eq!(
            resolve_web_asset_root(&executable, &development),
            Some(packaged.canonicalize().unwrap())
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn development_assets_are_fallback_when_not_packaged() {
        let base = std::env::temp_dir().join(format!(
            "mdvr-platform-assets-{}-development",
            std::process::id()
        ));
        let development = base.join("web/dist");
        fs::create_dir_all(&development).unwrap();
        fs::write(development.join("index.html"), "development").unwrap();

        let executable = base.join("target/release/mdvr");
        assert_eq!(
            resolve_web_asset_root(&executable, &development),
            Some(development.canonicalize().unwrap())
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn navigation_allows_only_exact_owned_entrypoint() {
        assert!(navigation_allowed_url("mdvr://localhost/index.html".into()));
        assert!(!navigation_allowed_url(
            "mdvr://localhost/other.html".into()
        ));
        assert!(!navigation_allowed_url("https://example.com".into()));
    }

    #[test]
    fn custom_protocol_rejects_traversal_and_symlink_escape() {
        let base = std::env::temp_dir().join(format!("mdvr-protocol-{}", std::process::id()));
        let root = base.join("web");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("index.html"), "safe").unwrap();
        fs::write(base.join("secret"), "secret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(base.join("secret"), root.join("escape")).unwrap();

        let root = root.canonicalize().unwrap();
        assert_eq!(
            asset_path(&root, "/index.html"),
            Some(root.join("index.html"))
        );
        assert_eq!(asset_path(&root, "/../secret"), None);
        #[cfg(unix)]
        assert_eq!(asset_path(&root, "/escape"), None);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn document_load_script_json_encodes_source() {
        let source = "# quote \\\"\\n</script>\\u{2028}";
        let generation = crate::contracts::Generation::new(7).unwrap();
        let script = document_load_script(source, generation).unwrap();
        assert_eq!(
            script,
            format!(
                "window.mdvrLoadDocument({}, 7);",
                serde_json::to_string(source).unwrap()
            )
        );
        assert!(script.contains(r#"\\\""#));
        assert!(script.contains(r#"\\n"#));
        assert!(!script.contains(source));
    }

    #[test]
    fn document_generation_rejects_stale_source() {
        let mut generations = DocumentGeneration::default();
        let first = crate::contracts::Generation::new(2).unwrap();
        let stale = crate::contracts::Generation::new(2).unwrap();
        let older = crate::contracts::Generation::new(1).unwrap();
        let newer = crate::contracts::Generation::new(3).unwrap();

        assert!(generations.accept(first));
        assert!(!generations.accept(stale));
        assert!(!generations.accept(older));
        assert!(generations.accept(newer));
    }

    fn valid_appearance() -> Appearance {
        Appearance {
            mode: crate::contracts::AppearanceMode::Dark,
            scale_percent: 100,
            reader_background: "#111111".into(),
            reader_foreground: "#eeeeee".into(),
            code_background: "#222222".into(),
            accent: "#66aaff".into(),
            syntax: vec![crate::contracts::SyntaxToken {
                role: crate::contracts::SyntaxRole::Keyword,
                foreground: "#ff66aa".into(),
                background: None,
                bold: true,
                italic: false,
            }],
        }
    }

    #[test]
    fn appearance_script_json_encodes_contract_tokens() {
        let appearance = valid_appearance();
        let script = appearance_script(&appearance).unwrap();
        assert_eq!(
            script,
            format!(
                "window.mdvrApplyAppearance({});",
                serde_json::to_string(&appearance).unwrap()
            )
        );
        assert!(!script.contains("url("));
        assert!(!script.contains("<script>"));
    }

    #[test]
    fn appearance_script_rejects_unvalidated_tokens() {
        let mut appearance = valid_appearance();
        appearance.accent = "url(javascript:bad)".into();
        assert!(matches!(
            appearance_script(&appearance),
            Err(ContractError::InvalidColor(_))
        ));
    }

    #[test]
    fn appearance_generation_allows_same_generation_and_rejects_stale() {
        let mut document = DocumentGeneration::default();
        let first = Generation::new(2).unwrap();
        let older = Generation::new(1).unwrap();
        let newer = Generation::new(3).unwrap();
        assert!(document.accept(first));

        let mut appearance = AppearanceGeneration::default();
        assert!(appearance.accept(first, &document));
        assert!(appearance.accept(first, &document));
        assert!(!appearance.accept(older, &document));
        assert!(document.accept(newer));
        assert!(!appearance.accept(first, &document));
        assert!(appearance.accept(newer, &document));
    }

    #[test]
    fn bridge_reencodes_only_valid_closed_envelopes() {
        let valid = br#"{"revision":1,"message":{"kind":"launch.request","payload":{"invocation":1,"intent":"picker","caller_cwd":"/work","path":null,"ack_timeout_ms":500}}}"#;
        assert_eq!(canonical_bridge_message(valid).unwrap(), valid);

        for invalid in [
            br#"{"revision":1,"message":{"kind":"unknown","payload":{}}}"#.as_slice(),
            br#"{"revision":1,"message":{"kind":"launch.request","payload":{"invocation":1,"intent":"picker","caller_cwd":"/work","path":null,"ack_timeout_ms":500,"extra":true}}}"#.as_slice(),
            br#"{"revision":99,"message":{"kind":"launch.request","payload":{"invocation":1,"intent":"picker","caller_cwd":"/work","path":null,"ack_timeout_ms":500}}}"#.as_slice(),
            br#"{"revision":1,"message":{"kind":"render.ready","payload":{"document":0,"generation":1,"headings":[]}}}"#.as_slice(),
        ] {
            assert!(canonical_bridge_message(invalid).is_err());
        }

        assert!(canonical_bridge_message(&vec![b' '; MAX_FRAME_BYTES + 1]).is_err());
    }
}
