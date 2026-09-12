#[cfg(not(target_os = "macos"))]
compile_error!("mdvr requires macOS");

use gpui::{App, Application, Context, Render, Window, WindowOptions, div, prelude::*};

struct Bootstrap;

impl Render for Bootstrap {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child("mdvr — build bootstrap; document viewer not implemented")
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Bootstrap))
            .expect("failed to open mdvr window");
        cx.activate(true);
    });
}
