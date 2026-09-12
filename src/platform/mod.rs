#![allow(unexpected_cfgs)]

use std::sync::OnceLock;

use block::Block;
use cocoa::{
    appkit::{NSView, NSViewHeightSizable, NSViewWidthSizable},
    base::{id, nil},
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

extern "C" fn cancel_navigation(_: &Object, _: Sel, _: id, _: id, decision_handler: id) {
    if decision_handler.is_null() {
        return;
    }

    unsafe {
        (*(decision_handler as *mut Block<(isize,), ()>)).call((0,));
    }
}

fn navigation_delegate_class() -> Option<&'static Class> {
    static CLASS: OnceLock<Option<&'static Class>> = OnceLock::new();

    CLASS
        .get_or_init(|| {
            let protocol = Protocol::get("WKNavigationDelegate")?;
            let mut declaration = ClassDecl::new("MdvrNavigationDelegate", class!(NSObject))?;
            declaration.add_protocol(protocol);
            unsafe {
                declaration.add_method(
                    sel!(webView:decidePolicyForNavigationAction:decisionHandler:),
                    cancel_navigation as extern "C" fn(&Object, Sel, id, id, id),
                );
            }
            Some(declaration.register())
        })
        .as_ref()
        .copied()
}

pub struct EmbeddedWebView {
    view: id,
    navigation_delegate: id,
}

impl EmbeddedWebView {
    pub fn attach(window: &Window) -> Option<Self> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let parent = match handle.as_raw() {
            RawWindowHandle::AppKit(handle) => handle.ns_view.as_ptr() as id,
            _ => return None,
        };

        unsafe {
            let configuration: id = msg_send![class!(WKWebViewConfiguration), alloc];
            let configuration: id = msg_send![configuration, init];
            if configuration.is_null() {
                return None;
            }

            let data_store: id = msg_send![class!(WKWebsiteDataStore), nonPersistentDataStore];
            let _: () = msg_send![configuration, setWebsiteDataStore: data_store];

            let delegate_class = navigation_delegate_class()?;
            let navigation_delegate: id = msg_send![delegate_class, new];
            if navigation_delegate.is_null() {
                let _: () = msg_send![configuration, release];
                return None;
            }

            let view: id = msg_send![class!(WKWebView), alloc];
            let view: id = msg_send![view, initWithFrame: NSView::bounds(parent)
                configuration: configuration];
            let _: () = msg_send![configuration, release];
            if view.is_null() {
                let _: () = msg_send![navigation_delegate, release];
                return None;
            }

            // Delegate is weak on WKWebView; retain it in EmbeddedWebView.
            let _: () = msg_send![view, setNavigationDelegate: navigation_delegate];
            NSView::setAutoresizingMask_(view, NSViewWidthSizable | NSViewHeightSizable);
            NSView::addSubview_(parent, view);

            Some(Self {
                view,
                navigation_delegate,
            })
        }
    }

    /// Load bundled fixture from app-owned startup path. No bridge or document
    /// renderer integration is implied by this fixture load.
    pub fn load_initial_document(&self) {
        unsafe {
            let html = NSString::alloc(nil).init_str(include_str!("../../web/index.html"));
            let _: id = msg_send![self.view, loadHTMLString: html baseURL: nil];
            let _: () = msg_send![html, release];
        }
    }
}

impl Drop for EmbeddedWebView {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![self.view, setNavigationDelegate: nil];
            NSView::removeFromSuperview(self.view);
            let _: () = msg_send![self.view, release];
            let _: () = msg_send![self.navigation_delegate, release];
        }
    }
}
