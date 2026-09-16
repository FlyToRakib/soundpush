//! WebView2 runtime detection on Windows (plan §10.3 mitigation, G9 "white screen").
//!
//! Tauri draws the whole interface in the system webview. When the Microsoft Edge WebView2
//! runtime is missing or broken, creating the window fails and the user is left with nothing —
//! the failure AudioRelay shows as a white screen. The engine and the tray do not need a webview,
//! so SoundPush keeps running and says what is wrong instead.
//!
//! The runtime records its version under `EdgeUpdate\Clients\{F3017226-…}`; per-machine installs
//! write it to HKLM (and to the 32-bit view on 64-bit Windows), per-user ones to HKCU. An empty
//! or `0.0.0.0` version is how the runtime marks itself as not really there.
//!
//! The repair is the same Evergreen bootstrapper the installer uses
//! (`bundle.windows.nsis` defaults to `downloadBootstrapper`), so nothing new is downloaded by
//! SoundPush itself: the link opens in the browser and the user runs Microsoft's installer.

#[cfg(windows)]
use tracing::warn;

/// Microsoft's Evergreen WebView2 bootstrapper, the same download the installer uses.
#[cfg_attr(not(windows), allow(dead_code))]
pub const INSTALL_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

/// Installed WebView2 runtime version, or `None` when it is missing or unusable. Always `None`
/// off Windows, where the platform webview ships with the OS.
pub fn version() -> Option<String> {
    #[cfg(windows)]
    return windows::version();
    #[allow(unreachable_code)]
    None
}

/// Whether a window can be created at all. Other platforms always can: WebKitGTK and WKWebView
/// are part of the system, and a missing WebKitGTK is a broken package, not a normal state.
pub fn available() -> bool {
    #[cfg(windows)]
    return version().is_some();
    #[allow(unreachable_code)]
    true
}

/// Tell the user the interface cannot open and offer Microsoft's installer. Returns whether they
/// asked for it. Plain Windows message box: translations live in the webview, which is exactly
/// what is missing here, so this one string cannot go through `en.json`.
#[cfg_attr(not(windows), allow(unused_variables))]
pub fn offer_install(app: &tauri::AppHandle) -> bool {
    #[cfg(windows)]
    {
        if !windows::ask_to_install() {
            return false;
        }
        use tauri_plugin_opener::OpenerExt;
        if let Err(e) = app.opener().open_url(INSTALL_URL, None::<&str>) {
            warn!(error = %e, "could not open the WebView2 download page");
        }
        return true;
    }
    #[allow(unreachable_code)]
    false
}

#[cfg(windows)]
mod windows {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        IDYES, MB_ICONWARNING, MB_SETFOREGROUND, MB_SYSTEMMODAL, MB_YESNO, MessageBoxW,
    };
    use windows::core::{HSTRING, w};

    /// The Evergreen runtime's product id under `EdgeUpdate\Clients`.
    const CLIENT: &str =
        r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    const CLIENT_WOW: &str =
        r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

    fn registry_string(root: HKEY, path: &str) -> Option<String> {
        let (path, name) = (HSTRING::from(path), HSTRING::from("pv"));
        let mut buf = [0u16; 128];
        let mut size = std::mem::size_of_val(&buf) as u32;
        // SAFETY: the buffer and its size in bytes are passed together; the call only writes there.
        let status = unsafe {
            RegGetValueW(
                root,
                &path,
                &name,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if status.is_err() {
            return None;
        }
        let chars = (size as usize / 2).min(buf.len());
        let end = buf[..chars].iter().position(|&c| c == 0).unwrap_or(chars);
        Some(String::from_utf16_lossy(&buf[..end]))
    }

    pub(super) fn version() -> Option<String> {
        // Per-machine: a 64-bit Windows records the Evergreen runtime under WOW6432Node, an
        // ARM64 or 32-bit one directly. Then the per-user install.
        [
            (HKEY_LOCAL_MACHINE, CLIENT_WOW),
            (HKEY_LOCAL_MACHINE, CLIENT),
            (HKEY_CURRENT_USER, CLIENT),
        ]
        .into_iter()
        .find_map(|(root, path)| registry_string(root, path))
        .map(|v| v.trim().to_string())
        // A runtime that removed itself leaves the key behind with an empty or zero version.
        .filter(|v| !v.is_empty() && v != "0.0.0.0")
    }

    pub(super) fn ask_to_install() -> bool {
        let text = w!(
            "SoundPush cannot open its window because the Microsoft Edge WebView2 runtime is missing or damaged.\n\nSoundPush keeps running in the notification area, so streams that are already set up continue.\n\nOpen Microsoft's download page to install it?"
        );
        // SAFETY: both strings are static wide literals and no parent window is passed.
        let answer = unsafe {
            MessageBoxW(
                HWND::default(),
                text,
                w!("SoundPush"),
                MB_YESNO | MB_ICONWARNING | MB_SETFOREGROUND | MB_SYSTEMMODAL,
            )
        };
        answer == IDYES
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_runtime_version_is_read_without_admin() {
            // Every machine that builds SoundPush has the runtime (Tauri needs it), so a version
            // must come back, and it must look like one.
            let v = version().unwrap_or_default();
            assert!(
                v.split('.').count() >= 3 && v.chars().all(|c| c.is_ascii_digit() || c == '.'),
                "unexpected WebView2 version {v:?}"
            );
        }
    }
}
