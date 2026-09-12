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
    slice,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{
    contracts::{
        Appearance, AppearanceUpdate, ContractError, DocumentId, Envelope, Generation, Locator,
        MAX_FRAME_BYTES, Message, ResourceResult, decode, encode,
    },
    platform::bridge::{BridgeContext, BridgeMessage, BridgeRouter},
};
use block::Block;
use cocoa::{
    appkit::{NSView, NSViewHeightSizable, NSViewWidthSizable, NSWindowOrderingMode},
    base::{BOOL, id, nil},
    foundation::NSString,
};
use gpui::Window;
use objc::{
    class,
    declare::ClassDecl,
    msg_send,
    runtime::{Class, Object, Protocol, Sel},
    sel, sel_impl,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[link(name = "WebKit", kind = "framework")]
unsafe extern "C" {}

const BRIDGE_HANDLER_NAME: &str = "mdvr";
const PENDING_STATE_IVAR: &str = "_mdvr_pending_state";
const NS_UTF8_STRING_ENCODING: usize = 4;
static ACCEPTED_BRIDGE_MESSAGES: AtomicU64 = AtomicU64::new(0);
static REJECTED_BRIDGE_MESSAGES: AtomicU64 = AtomicU64::new(0);
static BRIDGE_ROUTER: OnceLock<Mutex<BridgeRouter>> = OnceLock::new();
static ALLOWED_DOCUMENT_PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

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

fn set_allowed_document_path(path: &Path) {
    if let Ok(mut allowed) = ALLOWED_DOCUMENT_PATH
        .get_or_init(|| Mutex::new(None))
        .lock()
    {
        *allowed = Some(path.to_path_buf());
    }
}

fn allowed_document_path() -> Option<PathBuf> {
    ALLOWED_DOCUMENT_PATH
        .get()
        .and_then(|allowed| allowed.lock().ok())
        .and_then(|allowed| allowed.clone())
}

fn navigation_path_allowed(candidate: &Path, expected: &Path) -> bool {
    candidate == expected
}

fn navigation_path(action: id) -> Option<PathBuf> {
    unsafe {
        let request: id = msg_send![action, request];
        if request.is_null() {
            return None;
        }
        let url: id = msg_send![request, URL];
        if url.is_null() {
            return None;
        }
        let is_file_url: objc::runtime::BOOL = msg_send![url, isFileURL];
        if is_file_url != objc::runtime::YES {
            return None;
        }
        let path: id = msg_send![url, path];
        if path.is_null() {
            return None;
        }
        let utf8: *const std::ffi::c_char = msg_send![path, UTF8String];
        if utf8.is_null() {
            return None;
        }
        CStr::from_ptr(utf8).to_str().ok().map(PathBuf::from)
    }
}

fn navigation_allowed(action: id) -> bool {
    let Some(expected) = allowed_document_path() else {
        return false;
    };
    let Some(candidate) = navigation_path(action) else {
        return false;
    };
    navigation_path_allowed(&candidate, &expected)
}

fn file_url(path: &Path, is_directory: bool) -> Option<id> {
    let path = path.to_str()?;
    unsafe {
        let path = NSString::alloc(nil).init_str(path);
        let directory = if is_directory {
            objc::runtime::YES
        } else {
            objc::runtime::NO
        };
        let url: id = msg_send![class!(NSURL), fileURLWithPath:path isDirectory:directory];
        let _: () = msg_send![path, release];
        (!url.is_null()).then_some(url)
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
        opened
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
    if !main_thread() || url.contains(char::is_control) {
        return None;
    }
    unsafe {
        let value = NSString::alloc(nil).init_str(url);
        let target: id = msg_send![class!(NSURL), URLWithString: value];
        let _: () = msg_send![value, release];
        if target.is_null() {
            return None;
        }
        let is_file: BOOL = msg_send![target, isFileURL];
        if !is_file {
            return None;
        }
        let path: id = msg_send![target, path];
        let bytes: *const std::os::raw::c_char = msg_send![path, UTF8String];
        (!bytes.is_null()).then(|| PathBuf::from(CStr::from_ptr(bytes).to_string_lossy().as_ref()))
    }
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
        opened
    }
}

pub(crate) fn update_bridge_context(context: BridgeContext) {
    let router = BRIDGE_ROUTER.get_or_init(|| Mutex::new(BridgeRouter::new(context)));
    if let Ok(mut router) = router.lock() {
        router.set_context(context);
    }
}

fn script_message_bytes(message: id) -> Option<Vec<u8>> {
    unsafe {
        if message.is_null() {
            return None;
        }

        let body: id = msg_send![message, body];
        if body.is_null() {
            return None;
        }
        let is_string: objc::runtime::BOOL = msg_send![body, isKindOfClass: class!(NSString)];
        if is_string != objc::runtime::YES {
            return None;
        }

        let data: id = msg_send![body, dataUsingEncoding: NS_UTF8_STRING_ENCODING];
        if data.is_null() {
            return None;
        }
        let length: usize = msg_send![data, length];
        if length > MAX_FRAME_BYTES {
            return None;
        }
        let bytes: *const u8 = msg_send![data, bytes];
        if bytes.is_null() && length != 0 {
            return None;
        }
        Some(slice::from_raw_parts(bytes, length).to_vec())
    }
}

extern "C" fn receive_script_message(_: &Object, _: Sel, _: id, message: id) {
    if !main_thread() {
        reject_bridge_message();
        return;
    }

    let accepted = script_message_bytes(message)
        .and_then(|bytes| canonical_bridge_message(&bytes).ok())
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

fn bridge_handler_class() -> Option<&'static Class> {
    static CLASS: OnceLock<Option<&'static Class>> = OnceLock::new();

    CLASS
        .get_or_init(|| {
            let mut declaration = ClassDecl::new("MdvrScriptMessageHandler", class!(NSObject))?;
            // WebKit dispatches the selector even when protocol metadata is not registered.
            if let Some(protocol) = Protocol::get("WKScriptMessageHandler") {
                declaration.add_protocol(protocol);
            }
            unsafe {
                declaration.add_method(
                    sel!(userContentController:didReceiveScriptMessage:),
                    receive_script_message as extern "C" fn(&Object, Sel, id, id),
                );
            }
            Some(declaration.register())
        })
        .as_ref()
        .copied()
}

extern "C" fn decide_navigation(
    _: &Object,
    _: Sel,
    _: id,
    navigation_action: id,
    decision_handler: id,
) {
    if decision_handler.is_null() {
        return;
    }

    let allowed = navigation_allowed(navigation_action);
    eprintln!(
        "mdvr: WebKit navigation policy {}",
        if allowed { "allow" } else { "cancel" }
    );
    let policy = if allowed { 1 } else { 0 };
    unsafe {
        (*(decision_handler as *mut Block<(isize,), ()>)).call((policy,));
    }
}

extern "C" fn finish_navigation(delegate: &Object, _: Sel, web_view: id, _: id) {
    if !main_thread() {
        return;
    }
    eprintln!("mdvr: WebKit navigation finished");
    let state = unsafe { *delegate.get_ivar::<usize>(PENDING_STATE_IVAR) as *mut PendingPageState };
    if web_view.is_null() || state.is_null() {
        return;
    }
    unsafe {
        (*state).apply_pending(web_view);
    }
}

fn evaluate_javascript(view: id, script: &str) {
    if view.is_null() {
        return;
    }
    unsafe {
        let script = NSString::alloc(nil).init_str(script);
        let completion = block::ConcreteBlock::new(|_: id, error: id| {
            if !error.is_null() {
                // Exception details may contain document source; never log userInfo.
                let description: id = msg_send![error, localizedDescription];
                let text: *const std::ffi::c_char = msg_send![description, UTF8String];
                if !text.is_null() {
                    eprintln!(
                        "mdvr: JavaScript evaluation failed: {}",
                        CStr::from_ptr(text).to_string_lossy()
                    );
                }
            }
        })
        .copy();
        let _: () = msg_send![view,
            evaluateJavaScript: script
            completionHandler: &*completion];
        let _: () = msg_send![script, release];
    }
}

fn navigation_delegate_class() -> Option<&'static Class> {
    static CLASS: OnceLock<Option<&'static Class>> = OnceLock::new();

    CLASS
        .get_or_init(|| {
            let mut declaration = ClassDecl::new("MdvrNavigationDelegate", class!(NSObject))?;
            if let Some(protocol) = Protocol::get("WKNavigationDelegate") {
                declaration.add_protocol(protocol);
            }
            declaration.add_ivar::<usize>(PENDING_STATE_IVAR);
            unsafe {
                declaration.add_method(
                    sel!(webView:decidePolicyForNavigationAction:decisionHandler:),
                    decide_navigation as extern "C" fn(&Object, Sel, id, id, id),
                );
                declaration.add_method(
                    sel!(webView:didFinishNavigation:),
                    finish_navigation as extern "C" fn(&Object, Sel, id, id),
                );
            }
            Some(declaration.register())
        })
        .as_ref()
        .copied()
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

#[derive(Clone, Debug, Default, PartialEq)]
struct PendingPage {
    source: Option<PendingSource>,
    appearance: Option<PendingAppearance>,
    context: Option<PendingContext>,
    theme_choices: Option<String>,
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

    fn take_pending(&mut self) -> PendingPage {
        self.page_ready = true;
        PendingPage {
            source: self.source.take(),
            appearance: self.appearance.take(),
            context: self.context.take(),
            theme_choices: self.theme_choices.take(),
            locator: self.locator.take(),
        }
    }

    fn apply_pending(&mut self, web_view: id) {
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
    view: id,
    navigation_delegate: id,
    bridge_handler: id,
    current_generation: DocumentGeneration,
    #[allow(dead_code)]
    appearance_generation: AppearanceGeneration,
    pending_state: Box<PendingPageState>,
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
        assert!(main_thread(), "WKWebView must be used on main thread");
        unsafe {
            let window: id = msg_send![self.view, window];
            if window.is_null() {
                return false;
            }
            let accepted: BOOL = msg_send![window, makeFirstResponder: self.view];
            accepted
        }
    }

    pub fn sync_frame(&self) {
        assert!(main_thread(), "WKWebView must be used on main thread");
        unsafe {
            let bounds = NSView::bounds(self.parent);
            let _: () = msg_send![self.view, setFrame: bounds];
            let _: () = msg_send![self.parent,
                addSubview: self.view
                positioned: NSWindowOrderingMode::NSWindowAbove
                relativeTo: nil];
        }
    }

    pub fn attach(window: &Window) -> Option<Self> {
        if !main_thread() {
            return None;
        }
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let parent = match handle.as_raw() {
            RawWindowHandle::AppKit(handle) => handle.ns_view.as_ptr() as id,
            _ => return None,
        };

        unsafe {
            let initial_bounds = NSView::bounds(parent);
            eprintln!(
                "mdvr: WebKit initial frame {}x{}",
                initial_bounds.size.width, initial_bounds.size.height
            );
            let configuration: id = msg_send![class!(WKWebViewConfiguration), alloc];
            let configuration: id = msg_send![configuration, init];
            if configuration.is_null() {
                return None;
            }

            let data_store: id = msg_send![class!(WKWebsiteDataStore), nonPersistentDataStore];
            let _: () = msg_send![configuration, setWebsiteDataStore: data_store];

            let Some(bridge_class) = bridge_handler_class() else {
                eprintln!("mdvr: script-message handler class unavailable");
                let _: () = msg_send![configuration, release];
                return None;
            };
            let bridge_handler: id = msg_send![bridge_class, new];
            if bridge_handler.is_null() {
                let _: () = msg_send![configuration, release];
                return None;
            }
            let user_content_controller: id = msg_send![configuration, userContentController];
            if user_content_controller.is_null() {
                let _: () = msg_send![bridge_handler, release];
                let _: () = msg_send![configuration, release];
                return None;
            }
            let bridge_name = NSString::alloc(nil).init_str(BRIDGE_HANDLER_NAME);
            let _: () = msg_send![user_content_controller,
                addScriptMessageHandler: bridge_handler
                name: bridge_name];
            let _: () = msg_send![bridge_name, release];

            let Some(delegate_class) = navigation_delegate_class() else {
                eprintln!("mdvr: navigation delegate class unavailable");
                let _: () = msg_send![bridge_handler, release];
                let _: () = msg_send![configuration, release];
                return None;
            };
            let navigation_delegate: id = msg_send![delegate_class, new];
            if navigation_delegate.is_null() {
                let _: () = msg_send![bridge_handler, release];
                let _: () = msg_send![configuration, release];
                return None;
            }

            let view: id = msg_send![class!(WKWebView), alloc];
            let view: id = msg_send![view, initWithFrame: initial_bounds
                configuration: configuration];
            let _: () = msg_send![configuration, release];
            if view.is_null() {
                let _: () = msg_send![navigation_delegate, release];
                let _: () = msg_send![bridge_handler, release];
                return None;
            }

            let mut pending_state = Box::new(PendingPageState::default());
            let pending_state_ptr = pending_state.as_mut() as *mut PendingPageState as usize;
            // Delegate is weak on WKWebView; retain it in EmbeddedWebView.
            (*navigation_delegate).set_ivar(PENDING_STATE_IVAR, pending_state_ptr);
            let _: () = msg_send![view, setNavigationDelegate: navigation_delegate];
            NSView::setAutoresizingMask_(view, NSViewWidthSizable | NSViewHeightSizable);
            let _: () = msg_send![parent,
                addSubview: view
                positioned: NSWindowOrderingMode::NSWindowAbove
                relativeTo: nil];

            Some(Self {
                parent,
                view,
                navigation_delegate,
                bridge_handler,
                current_generation: DocumentGeneration::default(),
                appearance_generation: AppearanceGeneration::default(),
                pending_state,
            })
        }
    }

    /// Load production web assets from the packaged app or development checkout.
    /// WebKit may read only this canonical app-owned asset directory.
    pub fn load_initial_document(&mut self) -> bool {
        assert!(main_thread(), "WKWebView must be used on main thread");
        let executable = match std::env::current_exe() {
            Ok(executable) => executable,
            Err(error) => {
                eprintln!("mdvr: cannot locate executable for web assets: {error}");
                return false;
            }
        };
        let development_root = Path::new(DEV_WEB_ASSET_ROOT);
        let Some(root) = resolve_web_asset_root(&executable, development_root) else {
            eprintln!("mdvr: production web assets missing");
            return false;
        };
        let entry = root.join("index.html");
        set_allowed_document_path(&entry);
        unsafe {
            let Some(entry_url) = file_url(&entry, false) else {
                return false;
            };
            let Some(root_url) = file_url(&root, true) else {
                return false;
            };
            self.pending_state.begin_load();
            let _: id = msg_send![self.view,
                loadFileURL: entry_url
                allowingReadAccessToURL: root_url];
        }
        true
    }

    /// Push trusted native source into the already-loaded bundled renderer.
    /// Stale generations leave the current rendered document untouched.
    pub fn load_document_source(
        &mut self,
        source: &str,
        generation: Generation,
    ) -> Result<bool, serde_json::Error> {
        assert!(main_thread(), "WKWebView must be used on main thread");
        let script = document_load_script(source, generation)?;
        if !self.current_generation.accept(generation) {
            return Ok(false);
        }

        if self.pending_state.page_ready() {
            self.pending_state.advance_source_generation(generation);
            evaluate_javascript(self.view, &script);
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
        assert!(main_thread(), "WKWebView must be used on main thread");
        let context = BridgeContext {
            document,
            generation,
        };
        if !self.pending_state.replace_context(context) {
            return Ok(());
        }
        if self.pending_state.page_ready() {
            evaluate_javascript(self.view, &navigation_context_script(document, generation));
        }
        Ok(())
    }

    pub fn navigate_anchor(&self, anchor: &str) -> Result<(), serde_json::Error> {
        assert!(main_thread(), "WKWebView must be used on main thread");
        let script = anchor_script(anchor)?;
        if self.pending_state.page_ready() {
            evaluate_javascript(self.view, &script);
        }
        Ok(())
    }

    pub fn restore_locator(
        &mut self,
        locator: Locator,
        generation: Generation,
    ) -> Result<(), serde_json::Error> {
        assert!(main_thread(), "WKWebView must be used on main thread");
        let script = locator_script(&locator)?;
        if self.pending_state.page_ready() {
            evaluate_javascript(self.view, &script);
        } else {
            self.pending_state.replace_locator(locator, generation);
        }
        Ok(())
    }

    pub fn set_theme_choices(&mut self, names: &[String], selected: Option<&str>) {
        assert!(main_thread(), "WKWebView must be used on main thread");
        let names = serde_json::to_string(names).expect("theme names serialize");
        let selected = serde_json::to_string(&selected).expect("theme selection serializes");
        let script = format!("window.mdvrSetThemeChoices({names}, {selected});");
        if self.pending_state.page_ready() {
            evaluate_javascript(self.view, &script);
        } else {
            self.pending_state.theme_choices = Some(script);
        }
    }

    pub fn set_history_availability(&self, back: bool, forward: bool) {
        assert!(main_thread(), "WKWebView must be used on main thread");
        if self.pending_state.page_ready() {
            evaluate_javascript(
                self.view,
                &format!("window.mdvrSetHistoryAvailability({back}, {forward});"),
            );
        }
    }

    pub fn deliver_resource(&self, result: ResourceResult) -> Result<(), ContractError> {
        assert!(main_thread(), "WKWebView must be used on main thread");
        let json = String::from_utf8(encode(&Envelope::new(Message::ResourceResult(result)))?)
            .expect("JSON encoding is UTF-8");
        evaluate_javascript(
            self.view,
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
        assert!(main_thread(), "WKWebView must be used on main thread");
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
            evaluate_javascript(self.view, &script);
        }
        Ok(true)
    }
}

impl Drop for EmbeddedWebView {
    fn drop(&mut self) {
        assert!(main_thread(), "WKWebView must be torn down on main thread");
        unsafe {
            let configuration: id = msg_send![self.view, configuration];
            let user_content_controller: id = msg_send![configuration, userContentController];
            if !user_content_controller.is_null() {
                let bridge_name = NSString::alloc(nil).init_str(BRIDGE_HANDLER_NAME);
                let _: () = msg_send![user_content_controller,
                    removeScriptMessageHandlerForName: bridge_name];
                let _: () = msg_send![bridge_name, release];
            }
            let _: () = msg_send![self.view, setNavigationDelegate: nil];
            (*self.navigation_delegate).set_ivar(PENDING_STATE_IVAR, 0usize);
            NSView::removeFromSuperview(self.view);
            let _: () = msg_send![self.view, release];
            let _: () = msg_send![self.bridge_handler, release];
            let _: () = msg_send![self.navigation_delegate, release];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn webkit_callback_classes_register_without_requiring_protocol_metadata() {
        let handler = bridge_handler_class().expect("script handler class");
        assert!(
            handler
                .instance_method(sel!(userContentController:didReceiveScriptMessage:))
                .is_some()
        );
        let delegate = navigation_delegate_class().expect("navigation delegate class");
        assert!(
            delegate
                .instance_method(sel!(webView:didFinishNavigation:))
                .is_some()
        );
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
        assert!(!state.page_ready());
        let pending = state.take_pending();
        assert_eq!(pending.source.unwrap().source, "new");
        assert_eq!(pending.locator.unwrap().locator, locator);
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
        let entry = Path::new("/Applications/mdvr.app/Contents/Resources/web/index.html");
        assert!(navigation_path_allowed(entry, entry));
        assert!(!navigation_path_allowed(
            Path::new("/Applications/mdvr.app/Contents/Resources/web/other.html"),
            entry
        ));
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
