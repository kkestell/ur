//! Keeps the app in the background on macOS while the end-to-end suite drives
//! it: it never becomes the active application, and its window cannot be seen
//! or clicked, but its webview keeps rendering.

use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use tauri::{ActivationPolicy, App, Manager};

/// `NSApplicationActivationPolicyAccessory`: no Dock icon, and windows can be
/// shown.
const ACCESSORY: isize = 1;

/// Prevents the app from becoming active when it launches. Call before the app
/// runs.
pub fn prohibit_activation(app: &mut App) {
    app.set_activation_policy(ActivationPolicy::Prohibited);
}

/// Shows the main window without activating the app: transparent, letting
/// clicks pass through, and in front so WebKit keeps the page visible. The
/// webview also ignores whether other windows cover its window.
pub fn show_main_window(app: &App) -> tauri::Result<()> {
    let window = app
        .get_webview_window("main")
        .expect("the main window exists");
    let ns_window = window.ns_window()?.cast::<AnyObject>();
    // SAFETY: `ns_window` is the window's live `NSWindow`, and setup runs on
    // the main thread.
    unsafe {
        // A prohibited app cannot show windows, and the launch is over, so the
        // app no longer activates.
        let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: bool = msg_send![ns_app, setActivationPolicy: ACCESSORY];
        let _: () = msg_send![ns_window, setAlphaValue: 0.0f64];
        let _: () = msg_send![ns_window, setIgnoresMouseEvents: true];
        let _: () = msg_send![ns_window, orderFrontRegardless];
    }
    window.with_webview(|webview| {
        let web_view = webview.inner().cast::<AnyObject>();
        // SAFETY: `web_view` is the live `WKWebView`, and `with_webview` runs
        // on the main thread. The setting is WebKit SPI.
        unsafe {
            let _: () = msg_send![web_view, _setWindowOcclusionDetectionEnabled: false];
        }
    })
}
