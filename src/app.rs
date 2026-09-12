use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use gpui::{
    App, Application, Context, Render, Task, Timer, Window, WindowAppearance, WindowOptions, div,
    prelude::*,
};

use crate::{
    LaunchPlan,
    contracts::{
        ActionMessage, ActionMessageEnvelope, ErrorCode, Generation, NavigationRequest,
        NavigationTarget, ResourceReference, ResourceRequest, ResourceResult, ResourceResultValue,
        SearchAction,
    },
    files::{LoadedSource, ReloadOutcome, load_source, spawn_reload_worker},
    navigation::{LoadRequest, Locator, NavigationAction, NavigationState},
    platform::{
        EmbeddedWebView,
        bridge::{BridgeContext, BridgeMessage},
        drain_bridge_messages,
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
        ActionMessage::Outline(_)
        | ActionMessage::CapturePosition(_)
        | ActionMessage::RestorePosition(_) => unreachable!("router filters bridge actions"),
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
    bridge_task: Task<()>,
    resource_policy: Option<ResourcePolicy>,
    preferences: Preferences,
}

impl MdvrView {
    fn new(shell: ShellState, navigation: NavigationState, cx: &mut Context<Self>) -> Self {
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
            bridge_task,
            resource_policy: None,
            preferences: Preferences::default(),
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

    fn initialize(&mut self, window: &Window, launch: &LaunchPlan, cx: &mut Context<Self>) {
        self.preferences = conventional_path()
            .map(|path| load_or_default(&path).preferences)
            .unwrap_or_default();
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
        let mode = match window.appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => AppearanceMode::Dark,
            WindowAppearance::Light | WindowAppearance::VibrantLight => AppearanceMode::Light,
        };
        if let Some(web_view) = self.web_view.as_mut()
            && let Some(generation) = context.generation
        {
            match default_theme(mode)
                .tokens
                .with_scale(self.preferences.text_scale_percent)
                .and_then(|tokens| tokens.as_revision_one())
            {
                Ok(appearance) => {
                    if let Err(error) = web_view.apply_appearance(&appearance, generation) {
                        eprintln!("mdvr: cannot apply appearance: {error}");
                    }
                }
                Err(error) => eprintln!("mdvr: invalid saved appearance: {error}"),
            }
        }
        if let Some(web_view) = self.web_view.as_ref() {
            web_view.sync_frame();
        }
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
        if let Some(web_view) = self.web_view.as_mut()
            && let Err(error) =
                web_view.set_navigation_context(context.document, context.generation)
        {
            eprintln!("mdvr: cannot update renderer navigation context: {error}");
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
                    let _ = dispatch_bridge_action(&mut self.shell, self.bridge_context, &action);
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
            Ok(NavigationAction::External(target)) => {
                eprintln!("mdvr: navigation delegated to native policy: {target:?}");
            }
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

    #[allow(dead_code)]
    fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Ok(Some(request)) = self.navigation.go_back(Locator::start()) {
            self.load_navigation(request, cx);
        }
    }

    #[allow(dead_code)]
    fn go_forward(&mut self, cx: &mut Context<Self>) {
        if let Ok(Some(request)) = self.navigation.go_forward(Locator::start()) {
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
        let _ = &self.bridge_task;
    }
}

impl Render for MdvrView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // Keep shell state native-owned while the WebKit view remains the only
        // interactive surface in this composition slice.
        let _renderer_has_focus = self.shell.focus.owner() == FocusOwner::Renderer;
        if let Some(web_view) = self.web_view.as_ref() {
            web_view.sync_frame();
        }
        if self.web_view.is_some() {
            div().size_full()
        } else {
            div().size_full().child("WKWebView attachment failed")
        }
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
