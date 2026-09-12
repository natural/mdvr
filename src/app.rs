use gpui::{App, Application, Context, Render, Window, WindowOptions, div, prelude::*};

use crate::{
    LaunchPlan,
    platform::EmbeddedWebView,
    ui::{FocusOwner, ShellState},
};

struct MdvrView {
    shell: ShellState,
    web_view: Option<EmbeddedWebView>,
}

impl Render for MdvrView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // Keep shell state native-owned while the WebKit view remains the only
        // interactive surface in this composition slice.
        let _renderer_has_focus = self.shell.focus.owner() == FocusOwner::Renderer;
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
        let opened = cx.open_window(WindowOptions::default(), move |window, cx| {
            let web_view = EmbeddedWebView::attach(window);
            if let Some(web_view) = web_view.as_ref()
                && launch_for_window.state.document.is_none()
            {
                // Picker fixture remains available until document bridge wiring lands.
                web_view.load_initial_document();
            }
            let mut shell = ShellState::new();
            shell.root = Some(launch_for_window.picker_root.clone());
            shell.current_document = launch_for_window.state.document.clone();
            cx.new(|_| MdvrView { shell, web_view })
        });
        match opened {
            Ok(_) => {
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
