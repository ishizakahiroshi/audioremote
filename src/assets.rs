//! The Web UI, embedded into the exe at build time.
//!
//! Shared rather than kept private to the HTTP server: the language packs under
//! `web/lang/` are also the single source of truth for the *host* side strings
//! (the tray). One embed, one set of translation files — the alternative is two
//! copies of the same Japanese that drift apart.

use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/"]
pub struct WebAssets;

/// The `.ico` already shipped for the browser tab and the exe resource.
///
/// `include_bytes!` rather than through [`WebAssets`]: the tray needs it before
/// the HTTP server exists, and a decoding path that cannot fail at runtime is
/// worth more here than the shared lookup.
const ICON_BYTES: &[u8] = include_bytes!("../web/icons/favicon.ico");

/// Decode the embedded icon at a specific pixel size.
///
/// An `.ico` is a directory of images, so the right entry has to be looked up
/// first: handing the whole file to `CreateIconFromResourceEx` yields either
/// nothing or the 256px entry squeezed into 16 pixels.
///
/// Returns an invalid handle rather than an error — every caller's fallback is
/// "carry on without an icon", and none of them can do anything more useful.
#[cfg(windows)]
pub fn load_icon(cx: i32, cy: i32) -> windows::Win32::UI::WindowsAndMessaging::HICON {
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIconFromResourceEx, LookupIconIdFromDirectoryEx, HICON, LR_DEFAULTCOLOR,
    };

    unsafe {
        let offset =
            LookupIconIdFromDirectoryEx(ICON_BYTES.as_ptr(), true, cx, cy, LR_DEFAULTCOLOR);
        if offset <= 0 || offset as usize >= ICON_BYTES.len() {
            return HICON::default();
        }
        let image = &ICON_BYTES[offset as usize..];
        // 0x00030000 is the icon resource format version every .ico uses.
        CreateIconFromResourceEx(image, true, 0x0003_0000, cx, cy, LR_DEFAULTCOLOR)
            .unwrap_or_default()
    }
}
