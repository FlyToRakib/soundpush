//! How the main window opens (plan §13.2 "Windowing"). Every open — first launch, the tray, a
//! second launch — shows the window in its normal state at the default size, centered on its
//! screen and never larger than the space beside the taskbar. Maximizing, snapping and resizing
//! last while the window is open; they are not carried into the next open.

use std::path::Path;

use tauri::{LogicalSize, WebviewWindow};
use tracing::warn;

/// Content size of a newly opened window, logical pixels.
pub const DEFAULT_SIZE: (f64, f64) = (1000.0, 700.0);
/// Below this the layout no longer fits, logical pixels.
pub const MIN_SIZE: (f64, f64) = (760.0, 520.0);

/// Where earlier builds remembered the window's size, position and maximized state.
const LEGACY_STATE_FILE: &str = "window.json";

/// Delete the geometry earlier builds saved; nothing reads it any more.
pub fn remove_legacy_state(data_dir: &Path) {
    match std::fs::remove_file(data_dir.join(LEGACY_STATE_FILE)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!(error = %e, "could not remove the old window state"),
    }
}

/// Return a window that was closed into memory ("Keep window in memory for instant reopen") to
/// how a new window opens, before it is shown again.
pub fn reset(window: &WebviewWindow) {
    // Measured before anything changes: size and state updates reach the window asynchronously.
    let size = window
        .current_monitor()
        .ok()
        .flatten()
        .and_then(|monitor| {
            let scale = monitor.scale_factor();
            let work_area = monitor.work_area().size.to_logical::<f64>(scale);
            let outer = window.outer_size().ok()?.to_logical::<f64>(scale);
            let inner = window.inner_size().ok()?.to_logical::<f64>(scale);
            Some(fit(
                (work_area.width, work_area.height),
                (outer.width - inner.width, outer.height - inner.height),
            ))
        })
        .unwrap_or(DEFAULT_SIZE);
    if window.is_minimized().unwrap_or(false) {
        let _ = window.unminimize();
    }
    if window.is_maximized().unwrap_or(false) {
        let _ = window.unmaximize();
    }
    let _ = window.set_size(LogicalSize::new(size.0, size.1));
    let _ = window.center();
}

/// The default content size, shrunk where it and the window frame would not fit in the work
/// area (small or highly scaled screens), as a new window is.
pub fn fit(work_area: (f64, f64), frame: (f64, f64)) -> (f64, f64) {
    let side = |default: f64, available: f64, frame: f64| default.min(available - frame.max(0.0));
    (
        side(DEFAULT_SIZE.0, work_area.0, frame.0).max(1.0),
        side(DEFAULT_SIZE.1, work_area.1, frame.1).max(1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: (f64, f64) = (16.0, 39.0);

    #[test]
    fn a_screen_with_room_gets_the_default_size() {
        assert_eq!(fit((2560.0, 1392.0), FRAME), DEFAULT_SIZE);
        // 3840 × 2160 at 150 %.
        assert_eq!(fit((2560.0, 1392.0), (15.0, 37.0)), DEFAULT_SIZE);
        assert_eq!(fit((1016.0, 739.0), FRAME), DEFAULT_SIZE);
    }

    #[test]
    fn a_small_screen_shrinks_the_window_to_fit_beside_the_taskbar() {
        // 1366 × 768 with a 40 px taskbar: 700 + 39 would run under the taskbar.
        assert_eq!(fit((1366.0, 728.0), FRAME), (1000.0, 689.0));
        // 1920 × 1080 at 200 %.
        assert_eq!(fit((960.0, 516.0), FRAME), (944.0, 477.0));
    }

    #[test]
    fn a_nonsensical_measurement_never_gives_an_empty_window() {
        assert_eq!(fit((0.0, 0.0), FRAME), (1.0, 1.0));
        assert_eq!(fit((2560.0, 1392.0), (-5.0, -5.0)), DEFAULT_SIZE);
    }

    #[test]
    fn a_leftover_state_file_is_removed_and_a_missing_one_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(LEGACY_STATE_FILE);
        std::fs::write(
            &file,
            br#"{"x":0,"y":0,"width":3840,"height":2088,"maximized":true}"#,
        )
        .unwrap();
        remove_legacy_state(dir.path());
        assert!(!file.exists());
        remove_legacy_state(dir.path());
    }
}
