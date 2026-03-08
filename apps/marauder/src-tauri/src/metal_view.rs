//! macOS-specific: ensure the WKWebView subview is transparent so the wgpu
//! Metal layer (set on the parent WryWebViewParent) shows through.

use std::ffi::c_void;

#[link(name = "objc", kind = "dylib")]
extern "C" {
    fn objc_getClass(name: *const i8) -> *mut c_void;
    fn sel_registerName(name: *const i8) -> *mut c_void;
    fn objc_msgSend(receiver: *mut c_void, sel: *mut c_void, ...) -> *mut c_void;
}

macro_rules! sel {
    ($name:expr) => {
        sel_registerName(concat!($name, "\0").as_ptr() as *const i8)
    };
}

/// After wgpu creates its surface on the window, ensure the WKWebView
/// subview is transparent so the Metal-rendered terminal grid shows through.
///
/// # Safety
/// Must be called on the main thread.
pub unsafe fn ensure_webview_transparency(window: &tauri::WebviewWindow) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let raw_handle = match window.window_handle() {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(error = %e, "Failed to get window handle for transparency fix");
            return;
        }
    };
    let parent_view = match raw_handle.as_raw() {
        RawWindowHandle::AppKit(h) => h.ns_view.as_ptr() as *mut c_void,
        _ => return,
    };

    // Find WKWebView among subviews and make it transparent
    let subviews: *mut c_void = objc_msgSend(parent_view, sel!("subviews"));
    let count: usize = {
        type CountFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize;
        let func: CountFn = std::mem::transmute(objc_msgSend as *const c_void);
        func(subviews, sel!("count"))
    };

    tracing::info!(subview_count = count, "Scanning subviews for WKWebView");

    for i in 0..count {
        let subview: *mut c_void = {
            type ObjectAtFn = unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void;
            let func: ObjectAtFn = std::mem::transmute(objc_msgSend as *const c_void);
            func(subviews, sel!("objectAtIndex:"), i)
        };

        let class_name: *mut c_void = objc_msgSend(subview, sel!("className"));
        let class_str: *const i8 = objc_msgSend(class_name, sel!("UTF8String")) as _;
        let name = std::ffi::CStr::from_ptr(class_str).to_string_lossy();
        tracing::info!(index = i, class = %name, "Subview found");

        if name.contains("WebView") || name.contains("Wry") {
            // setOpaque:NO
            {
                type SetBoolFn = unsafe extern "C" fn(*mut c_void, *mut c_void, i8);
                let func: SetBoolFn = std::mem::transmute(objc_msgSend as *const c_void);
                func(subview, sel!("setOpaque:"), 0);
            }

            // _setDrawsBackground:NO (private API, enabled via macOSPrivateApi)
            let responds: bool = {
                type RespondsFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> bool;
                let func: RespondsFn = std::mem::transmute(objc_msgSend as *const c_void);
                func(subview, sel!("respondsToSelector:"), sel!("_setDrawsBackground:"))
            };
            if responds {
                type SetBoolFn = unsafe extern "C" fn(*mut c_void, *mut c_void, i8);
                let func: SetBoolFn = std::mem::transmute(objc_msgSend as *const c_void);
                func(subview, sel!("_setDrawsBackground:"), 0);
                tracing::info!("Set _setDrawsBackground:NO on WKWebView");
            }

            // Traverse WKWebView inner subviews and set them non-opaque
            let inner_subviews: *mut c_void = objc_msgSend(subview, sel!("subviews"));
            let inner_count: usize = {
                type CountFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize;
                let func: CountFn = std::mem::transmute(objc_msgSend as *const c_void);
                func(inner_subviews, sel!("count"))
            };
            for j in 0..inner_count {
                let inner_view: *mut c_void = {
                    type ObjectAtFn = unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void;
                    let func: ObjectAtFn = std::mem::transmute(objc_msgSend as *const c_void);
                    func(inner_subviews, sel!("objectAtIndex:"), j)
                };
                // setOpaque:NO
                {
                    type SetBoolFn = unsafe extern "C" fn(*mut c_void, *mut c_void, i8);
                    let func: SetBoolFn = std::mem::transmute(objc_msgSend as *const c_void);
                    func(inner_view, sel!("setOpaque:"), 0);
                }
                // Also call _setDrawsBackground:NO if inner view supports it
                let inner_responds: bool = {
                    type RespondsFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> bool;
                    let func: RespondsFn = std::mem::transmute(objc_msgSend as *const c_void);
                    func(inner_view, sel!("respondsToSelector:"), sel!("_setDrawsBackground:"))
                };
                if inner_responds {
                    type SetBoolFn = unsafe extern "C" fn(*mut c_void, *mut c_void, i8);
                    let func: SetBoolFn = std::mem::transmute(objc_msgSend as *const c_void);
                    func(inner_view, sel!("_setDrawsBackground:"), 0);
                }

                let ic_name: *mut c_void = objc_msgSend(inner_view, sel!("className"));
                let ic_str: *const i8 = objc_msgSend(ic_name, sel!("UTF8String")) as _;
                let ic = std::ffi::CStr::from_ptr(ic_str).to_string_lossy();
                tracing::info!(index = j, class = %ic, "Inner subview set non-opaque");
            }

            tracing::info!("WKWebView transparency fully configured");

            tracing::info!("WebView transparency configured");
        }
    }

    // 3. Parent view: setOpaque:NO, setWantsLayer:YES
    {
        type SetBoolFn = unsafe extern "C" fn(*mut c_void, *mut c_void, i8);
        let func: SetBoolFn = std::mem::transmute(objc_msgSend as *const c_void);
        func(parent_view, sel!("setOpaque:"), 0);
        func(parent_view, sel!("setWantsLayer:"), 1);
    }

    tracing::info!("WebView transparency fix applied");
}
