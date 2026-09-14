//! The main window's size, position and maximized state across restarts (plan §13.2
//! "Windowing"). Stored in physical pixels in the data folder; restored only where it still fits
//! on a connected monitor, so an unplugged screen never hides the window.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{PhysicalPosition, PhysicalSize, WebviewWindow};
use tracing::warn;

const FILE: &str = "window.json";
/// How much of the window's top edge (where the title bar is) must be on a monitor.
const MIN_VISIBLE_WIDTH: i64 = 160;
const MIN_VISIBLE_HEIGHT: i64 = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

/// A monitor's work area (without taskbars and docks), physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub struct WindowState {
    path: PathBuf,
    /// Latest geometry and whether it differs from the file.
    current: Mutex<(Option<Geometry>, bool)>,
}

impl WindowState {
    pub fn load(data_dir: &std::path::Path) -> Self {
        let path = data_dir.join(FILE);
        let saved = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Geometry>(&b).ok())
            .filter(|g| g.width > 0 && g.height > 0);
        Self {
            path,
            current: Mutex::new((saved, false)),
        }
    }

    pub fn saved(&self) -> Option<Geometry> {
        self.current.lock().ok().and_then(|c| c.0)
    }

    /// Remember the window after a move or resize. While maximized only the flag changes, so
    /// un-maximizing after a restart returns to the size the user chose.
    pub fn track(&self, window: &WebviewWindow) {
        if window.is_minimized().unwrap_or(true) {
            return;
        }
        let maximized = window.is_maximized().unwrap_or(false);
        let Ok(mut current) = self.current.lock() else {
            return;
        };
        let next = if maximized {
            current.0.map(|g| Geometry {
                maximized: true,
                ..g
            })
        } else {
            match (window.outer_position(), window.inner_size()) {
                (Ok(position), Ok(size)) => Some(Geometry {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                    maximized: false,
                }),
                _ => current.0,
            }
        };
        if next != current.0 {
            *current = (next, true);
        }
    }

    /// Write the latest geometry if it changed. Called when the window closes and on exit.
    pub fn save(&self) {
        let Ok(mut current) = self.current.lock() else {
            return;
        };
        let (Some(geometry), true) = *current else {
            return;
        };
        match serde_json::to_vec(&geometry) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&self.path, bytes) {
                    warn!(error = %e, "could not save the window position");
                }
                current.1 = false;
            }
            Err(e) => warn!(error = %e, "could not save the window position"),
        }
    }

    /// Apply the saved geometry to a window that is not shown yet. Without one that fits,
    /// the window keeps its default size, centered.
    pub fn restore(&self, window: &WebviewWindow) {
        let Some(saved) = self.saved() else {
            return;
        };
        let areas: Vec<Area> = window
            .available_monitors()
            .unwrap_or_default()
            .iter()
            .map(|m| {
                let area = m.work_area();
                Area {
                    x: area.position.x,
                    y: area.position.y,
                    width: area.size.width,
                    height: area.size.height,
                }
            })
            .collect();
        let Some(g) = place(saved, &areas) else {
            return;
        };
        let _ = window.set_size(PhysicalSize::new(g.width, g.height));
        let _ = window.set_position(PhysicalPosition::new(g.x, g.y));
        if g.maximized {
            let _ = window.maximize();
        }
    }
}

/// Where to put a saved window: unchanged when its top edge is on a monitor, moved and shrunk
/// into the monitor it overlaps most when only partly visible, `None` when it is on no monitor.
pub fn place(saved: Geometry, areas: &[Area]) -> Option<Geometry> {
    // Overlap of the window's rows `top..top + height` with a monitor, in square pixels.
    let overlap = |a: &Area, height: i64| {
        let left = i64::from(saved.x).max(i64::from(a.x));
        let right =
            (i64::from(saved.x) + i64::from(saved.width)).min(i64::from(a.x) + i64::from(a.width));
        let top = i64::from(saved.y).max(i64::from(a.y));
        let bottom = (i64::from(saved.y) + height).min(i64::from(a.y) + i64::from(a.height));
        (right - left).max(0) * (bottom - top).max(0)
    };
    let (area, best) = areas
        .iter()
        .map(|a| (a, overlap(a, i64::from(saved.height))))
        .max_by_key(|(_, o)| *o)?;
    if best == 0 {
        return None;
    }
    let title_bar_visible = saved.y >= area.y
        && overlap(area, MIN_VISIBLE_HEIGHT) >= MIN_VISIBLE_WIDTH * MIN_VISIBLE_HEIGHT;
    if title_bar_visible && saved.width <= area.width && saved.height <= area.height {
        return Some(saved);
    }
    let width = saved.width.min(area.width);
    let height = saved.height.min(area.height);
    let max_x = i64::from(area.x) + i64::from(area.width) - i64::from(width);
    let max_y = i64::from(area.y) + i64::from(area.height) - i64::from(height);
    Some(Geometry {
        x: i64::from(saved.x).clamp(i64::from(area.x), max_x) as i32,
        y: i64::from(saved.y).clamp(i64::from(area.y), max_y) as i32,
        width,
        height,
        maximized: saved.maximized,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: Area = Area {
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
    };
    const RIGHT: Area = Area {
        x: 1920,
        y: 0,
        width: 2560,
        height: 1400,
    };

    fn at(x: i32, y: i32, width: u32, height: u32) -> Geometry {
        Geometry {
            x,
            y,
            width,
            height,
            maximized: false,
        }
    }

    #[test]
    fn a_window_that_fits_stays_where_it_was() {
        let g = at(200, 100, 1000, 700);
        assert_eq!(place(g, &[PRIMARY]), Some(g));
        let on_right = at(2500, 300, 1200, 800);
        assert_eq!(place(on_right, &[PRIMARY, RIGHT]), Some(on_right));
    }

    #[test]
    fn a_window_on_an_unplugged_monitor_is_not_restored() {
        assert_eq!(place(at(2500, 300, 1200, 800), &[PRIMARY]), None);
        assert_eq!(place(at(-3000, 0, 800, 600), &[PRIMARY]), None);
        assert_eq!(place(at(0, 0, 800, 600), &[]), None);
    }

    #[test]
    fn a_partly_visible_window_moves_onto_the_monitor() {
        // Title bar above the top of the screen.
        let placed = place(at(100, -300, 1000, 700), &[PRIMARY]).unwrap();
        assert_eq!((placed.x, placed.y), (100, 0));
        // Mostly off the right edge.
        let placed = place(at(1800, 100, 1000, 700), &[PRIMARY]).unwrap();
        assert_eq!((placed.x, placed.y, placed.width), (920, 100, 1000));
    }

    #[test]
    fn a_window_larger_than_the_monitor_shrinks_to_it() {
        let placed = place(
            Geometry {
                maximized: true,
                ..at(10, 10, 2560, 1400)
            },
            &[PRIMARY],
        )
        .unwrap();
        assert_eq!(
            placed,
            Geometry {
                x: 0,
                y: 0,
                width: 1920,
                height: 1040,
                maximized: true
            }
        );
    }
}
