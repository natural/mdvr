use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use gpui::{
    App, Application, Context, FocusHandle, KeyDownEvent, Render, Task, Timer, Window,
    WindowAppearance, WindowOptions, div, prelude::*,
};

use crate::{
    LaunchPlan,
    contracts::{
        ActionMessage, ActionMessageEnvelope, ErrorCode, Generation, NavigationRequest,
        NavigationTarget, ResourceReference, ResourceRequest, ResourceResult, ResourceResultValue,
        RootId, ScanId, SearchAction,
    },
    files::{
        DiscoveryEvent, DiscoveryScanner, LoadedSource, ReloadOutcome, load_source,
        spawn_reload_worker,
    },
    navigation::{LoadRequest, Locator, NavigationAction, NavigationState},
    platform::{
        EmbeddedWebView,
        bridge::{BridgeContext, BridgeMessage},
        drain_bridge_messages, open_external_url,
        remote_policy::{RemoteLimits, RemotePolicy},
        resource_policy::{ResourceAuthorization, ResourcePolicy},
        update_bridge_context,
    },
    preferences::{Preferences, conventional_path, load_or_default, save},
    theme::{AppearanceMode, default_theme},
    ui::{FocusOwner, ShellCommand, ShellState},
};

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
            "svg" => "image/svg+xml",
            _ => return None,
        }
        .to_owned(),
    )
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
        ActionMessage::History(_) => {}
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
    bridge_task: Task<()>,
    resource_policy: Option<ResourcePolicy>,
    preferences: Preferences,
    appearance_mode: Option<AppearanceMode>,
    picker_focus: FocusHandle,
}

impl MdvrView {
    fn new(shell: ShellState, navigation: NavigationState, cx: &mut Context<Self>) -> Self {
        let picker_focus = cx.focus_handle();
        let bridge_task = cx.spawn(async move |view, cx| {
            loop {
                Timer::after(Duration::from_millis(16)).await;
                if view
                    .update(cx, |view, cx| view.drain_bridge_messages(cx))
                    .is_err()
                {
                    return;
                }
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
            bridge_task,
            resource_policy: None,
            preferences: Preferences::default(),
            appearance_mode: None,
            picker_focus,
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
        self.preferences = conventional_path()
            .map(|path| load_or_default(&path).preferences)
            .unwrap_or_default();
        self.shell.text_scale_percent = self.preferences.text_scale_percent;
        if launch.state.document.is_none() {
            self.start_discovery(cx);
            window.focus(&self.picker_focus);
            return;
        }
        let Some(mut web_view) = EmbeddedWebView::attach(window) else {
            return;
        };
        let _ = web_view.load_initial_document();
        if let Some(path) = launch.state.document.as_deref() {
            match load_source(path, false) {
                Ok(source) => {
                    let generation = Generation::new(1).expect("nonzero generation");
                    if let Err(error) = web_view.load_document_source(&source.source, generation) {
                        eprintln!("mdvr: cannot prepare {}: {error}", path.display());
                    }
                    self.navigation.open_initial(source.clone());
                    self.watch_document(path.to_owned(), Some(source), cx);
                }
                Err(error) => eprintln!("mdvr: cannot load {}: {error}", path.display()),
            }
        }
        self.web_view = Some(web_view);
        let context = self
            .navigation
            .current()
            .map_or_else(BridgeContext::default, |document| BridgeContext {
                document: Some(document.document),
                generation: Some(document.generation),
            });
        self.document_committed(context);
        self.save_preferences();
        self.update_appearance(window);
        if let Some(web_view) = self.web_view.as_ref() {
            web_view.sync_frame();
            let _ = web_view.focus();
        }
    }

    fn start_discovery(&mut self, cx: &mut Context<Self>) {
        let root = self
            .shell
            .root
            .clone()
            .expect("picker always has a browsing root");
        let root_id = RootId::new(1).expect("nonzero root");
        let scan_id = ScanId::new(1).expect("nonzero scan");
        let discovery = DiscoveryScanner::new().spawn(root, root_id, scan_id);
        self.discovery_task = Some(cx.spawn(async move |view, cx| {
            loop {
                Timer::after(Duration::from_millis(25)).await;
                match discovery.try_next() {
                    Ok(Some(event)) => {
                        let complete = matches!(event, DiscoveryEvent::Complete(_));
                        if view
                            .update(cx, |view, cx| {
                                match event {
                                    DiscoveryEvent::Batch(batch) => {
                                        let _ =
                                            view.shell.picker.apply_batch(&batch, root_id, scan_id);
                                    }
                                    DiscoveryEvent::Complete(done) => {
                                        let _ = view.shell.picker.complete(&done, root_id, scan_id);
                                    }
                                    DiscoveryEvent::Error(error) => {
                                        let _ = view.shell.picker.fail(&error, root_id, scan_id);
                                    }
                                }
                                cx.notify();
                            })
                            .is_err()
                        {
                            return;
                        }
                        if complete {
                            return;
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        eprintln!("mdvr: discovery failed: {error}");
                        return;
                    }
                }
            }
        }));
    }

    fn handle_picker_key(&mut self, event: &KeyDownEvent, window: &Window, cx: &mut Context<Self>) {
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

    fn open_picker_document(&mut self, relative: String, window: &Window, cx: &mut Context<Self>) {
        let Some(root) = self.shell.root.as_deref() else {
            return;
        };
        let path = root.join(relative);
        let Ok(source) = load_source(&path, false) else {
            eprintln!("mdvr: cannot open {}", path.display());
            return;
        };
        let Some(mut web_view) = EmbeddedWebView::attach(window) else {
            eprintln!("mdvr: WKWebView attachment failed");
            return;
        };
        let _ = web_view.load_initial_document();
        let generation = Generation::new(1).expect("nonzero generation");
        if let Err(error) = web_view.load_document_source(&source.source, generation) {
            eprintln!("mdvr: cannot prepare {}: {error}", path.display());
            return;
        }
        self.navigation.open_initial(source.clone());
        self.shell.current_document = Some(path.clone());
        self.web_view = Some(web_view);
        self.watch_document(path, Some(source), cx);
        let current = self.navigation.current().expect("document was opened");
        self.document_committed(BridgeContext {
            document: Some(current.document),
            generation: Some(current.generation),
        });
        self.appearance_mode = None;
        self.update_appearance(window);
        self.save_preferences();
        if let Some(web_view) = self.web_view.as_ref() {
            let _ = web_view.focus();
        }
        cx.notify();
    }

    fn document_committed(&mut self, context: BridgeContext) {
        self.bridge_context = context;
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

    fn update_appearance(&mut self, window: &Window) {
        let mode = match window.appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => AppearanceMode::Dark,
            WindowAppearance::Light | WindowAppearance::VibrantLight => AppearanceMode::Light,
        };
        if self.appearance_mode == Some(mode) {
            return;
        }
        let Some(generation) = self.bridge_context.generation else {
            return;
        };
        let result = default_theme(mode)
            .tokens
            .with_scale(self.preferences.text_scale_percent)
            .and_then(|tokens| tokens.as_revision_one());
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
        match spawn_reload_worker(path.clone(), source, generation) {
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
                        match action.action {
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
            }
        }
    }

    fn dispatch_resource(&mut self, request: ResourceRequest) {
        let result = match (&mut self.resource_policy, &request.reference) {
            (Some(policy), ResourceReference::RelativePath { value }) => {
                match policy.authorize(std::path::Path::new(value)) {
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
            _ => ResourceResultValue::Denied {
                code: ErrorCode::Unsupported,
            },
        };
        if let Some(web_view) = self.web_view.as_ref()
            && let Err(error) = web_view.deliver_resource(ResourceResult {
                request: request.request,
                resource: request.resource,
                document: request.document,
                generation: request.generation,
                result,
            })
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
        let target = navigation_target_text(&request.target);
        match self
            .navigation
            .request_navigation_from(&target, request.generation, Locator::start())
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
                    let allowed = RemotePolicy::new(RemoteLimits::default())
                        .and_then(|policy| policy.authorize(&url))
                        .is_ok();
                    if !allowed || !open_external_url(&url) {
                        eprintln!("mdvr: external URL rejected");
                    }
                }
                NavigationTarget::Mailto { url } => {
                    if !valid_mailto(&url) || !open_external_url(&url) {
                        eprintln!("mdvr: mail URL rejected");
                    }
                }
                NavigationTarget::LocalFile { path } => {
                    eprintln!("mdvr: local non-Markdown file requires confirmation: {path}");
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
        match load_source(&path, false) {
            Ok(source) => {
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
            Err(error) => {
                let _ = self.navigation.fail_load(&request);
                eprintln!("mdvr: navigation load failed: {error}");
            }
        }
    }

    fn commit_document(&mut self, document: crate::contracts::DocumentId) {
        let Some(current) = self.navigation.current() else {
            return;
        };
        self.shell.current_document = Some(current.path.clone());
        self.document_committed(BridgeContext {
            document: Some(document),
            generation: Some(current.generation),
        });
        self.save_preferences();
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
        if let Ok(Some(request)) = self.navigation.go_back(Locator::start()) {
            self.load_navigation(request, cx);
        }
    }

    fn go_forward(&mut self, cx: &mut Context<Self>) {
        if let Ok(Some(request)) = self.navigation.go_forward(Locator::start()) {
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
                eprintln!(
                    "mdvr: reload generation {} failed: {error}",
                    request.generation.get()
                );
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
        let _ = &self.bridge_task;
    }
}

impl Render for MdvrView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.update_appearance(window);
        if let Some(web_view) = self.web_view.as_ref() {
            web_view.sync_frame();
            return div().size_full().into_any_element();
        }
        let selected = self.shell.picker.selected().map(str::to_owned);
        let entries = self.shell.picker.visible();
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
                    .text_color(gpui::rgb(0xffffff))
                    .text_xl()
                    .child("Choose a Markdown file"),
            )
            .child(
                div()
                    .text_color(gpui::rgb(0xb0b3b8))
                    .child(format!("Filter: {}", self.shell.picker.query())),
            )
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
                |view| view.child("No Markdown files found"),
            )
            .into_any_element()
    }
}

pub fn run(launch: LaunchPlan) {
    let launch_for_window = launch.clone();
    Application::new().run(move |cx: &mut App| {
        let opened = cx.open_window(WindowOptions::default(), move |_window, cx| {
            let mut shell = ShellState::new();
            shell.root = Some(launch_for_window.picker_root.clone());
            shell.current_document = launch_for_window.state.document.clone();
            let navigation = NavigationState::new(launch_for_window.picker_root.clone());
            cx.new(|cx| MdvrView::new(shell, navigation, cx))
        });
        match opened {
            Ok(window) => {
                let _ = window.update(cx, |view, native_window, cx| {
                    view.initialize(native_window, &launch, cx);
                    cx.notify();
                });
                if launch.explicit {
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
                std::process::exit(1);
            }
        }
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
    fn mailto_validation_is_bounded_and_blocks_control_characters() {
        assert!(valid_mailto("MAILTO:reader@example.com"));
        assert!(!valid_mailto(
            "mailto:reader@example.com\r\nBcc:x@example.com"
        ));
        assert!(!valid_mailto(&format!("mailto:{}", "x".repeat(2048))));
    }

    #[test]
    fn resource_mime_allows_only_static_image_formats() {
        assert_eq!(
            resource_mime("diagram.SVG").as_deref(),
            Some("image/svg+xml")
        );
        assert_eq!(resource_mime("photo.jpeg").as_deref(), Some("image/jpeg"));
        assert_eq!(resource_mime("animated.gif"), None);
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
