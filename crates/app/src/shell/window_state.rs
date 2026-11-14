//! The window's saved state (ROADMAP M3.1; `docs/ARCHITECTURE.md` →
//! "Decision record: the app shell and window state"): where the window was,
//! how it was shown and which view it had open, kept in the database's
//! `setting` table ([`coffer_core::setting::WindowState`]).
//!
//! The window state is a convenience, so nothing here stops the app: a
//! database that can't be opened, a restore in progress or a stored value
//! that doesn't parse leaves the window at its defaults, with a warning on
//! stderr, and a failed save is dropped the same way. Like the MCP server,
//! the app holds no connection between reads and writes ([`WindowStore`]
//! opens the database for each), so a restore can swap the file.
//!
//! Restoring checks the saved bounds against the displays found now: a
//! window whose title bar would be off every display is centred on the
//! primary display instead, and one smaller than the window minimum grows
//! to it. Both happen before the window opens, in [`restored_bounds`], which
//! needs no window and is unit tested.

use std::error::Error;
use std::path::PathBuf;

use coffer_core::backup;
use coffer_core::db::Database;
use coffer_core::setting::{self, WindowMode, WindowState};
use gpui_kit::{Bounds, Pixels, WindowBounds, point, px, size};

use crate::theme::tokens::size as token_size;

use super::View;

/// Where the window state is read from and saved to: the database at
/// `database`, opened for each read or write.
#[derive(Clone, Debug)]
pub struct WindowStore {
    database: PathBuf,
}

impl WindowStore {
    /// The store in the database at `database`, which is created when the
    /// app first saves or reads (`Database::open`).
    pub fn new(database: PathBuf) -> Self {
        Self { database }
    }

    /// The saved state, or `None` (with a warning) when there is none or it
    /// can't be read.
    pub fn load(&self) -> Option<WindowState> {
        match self.read() {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(%error, "couldn't read the window state; using the defaults");
                None
            }
        }
    }

    /// Saves `state`, replacing the last one. A failure is logged and
    /// dropped: the next save tries again.
    pub fn save(&self, state: &WindowState) {
        if let Err(error) = self.write(state) {
            tracing::warn!(%error, "couldn't save the window state");
        }
    }

    fn read(&self) -> Result<Option<WindowState>, Box<dyn Error + Send + Sync>> {
        if backup::restoring(&self.database)? {
            return Ok(None);
        }
        let db = Database::open(&self.database)?;
        Ok(setting::window_state(&db.reader()?)?)
    }

    fn write(&self, state: &WindowState) -> Result<(), Box<dyn Error + Send + Sync>> {
        if backup::restoring(&self.database)? {
            // The restored database is the user's choice; this state can
            // wait for the next save.
            return Ok(());
        }
        let mut db = Database::open(&self.database)?;
        setting::set_window_state(&mut db, state)?;
        Ok(())
    }
}

/// The window's state as saved: its bounds, rounded to whole logical
/// pixels and kept in [`WindowState`]'s range, and the open `view`.
pub fn snapshot(bounds: WindowBounds, view: View) -> WindowState {
    let (mode, bounds) = match bounds {
        WindowBounds::Windowed(bounds) => (WindowMode::Windowed, bounds),
        WindowBounds::Maximized(bounds) => (WindowMode::Maximized, bounds),
        WindowBounds::Fullscreen(bounds) => (WindowMode::Fullscreen, bounds),
    };
    let extent = WindowState::MAX_EXTENT as f32;
    let coordinate = |value: Pixels| f32::from(value).round().clamp(-extent, extent) as i32;
    let side = |value: Pixels| f32::from(value).round().clamp(1., extent) as u32;
    WindowState {
        mode,
        x: coordinate(bounds.origin.x),
        y: coordinate(bounds.origin.y),
        width: side(bounds.size.width),
        height: side(bounds.size.height),
        view: Some(view.key().to_owned()),
    }
}

/// The bounds to open the window with: `saved`'s when its title bar lands
/// on one of `displays` (each display's bounds), grown to the window
/// minimum, else `fallback` (centred on the primary display) windowed.
///
/// With no displays reported (some platforms can't say where a window is),
/// the saved bounds are trusted.
pub fn restored_bounds(
    saved: Option<&WindowState>,
    displays: &[Bounds<Pixels>],
    fallback: Bounds<Pixels>,
) -> WindowBounds {
    let Some(saved) = saved else {
        return WindowBounds::Windowed(fallback);
    };
    let bounds = Bounds {
        origin: point(px(saved.x as f32), px(saved.y as f32)),
        size: size(
            px(saved.width as f32).max(token_size::WINDOW_MIN_WIDTH),
            px(saved.height as f32).max(token_size::WINDOW_MIN_HEIGHT),
        ),
    };
    if !displays.is_empty()
        && !displays
            .iter()
            .any(|display| title_bar_on(bounds, *display))
    {
        return WindowBounds::Windowed(fallback);
    }
    match saved.mode {
        WindowMode::Windowed => WindowBounds::Windowed(bounds),
        WindowMode::Maximized => WindowBounds::Maximized(bounds),
        WindowMode::Fullscreen => WindowBounds::Fullscreen(bounds),
    }
}

/// Whether enough of the title bar of a window at `bounds` is on `display`
/// to grab and drag: a title bar's height in both directions.
fn title_bar_on(bounds: Bounds<Pixels>, display: Bounds<Pixels>) -> bool {
    let title_bar = Bounds {
        origin: bounds.origin,
        size: size(bounds.size.width, token_size::TITLE_BAR),
    };
    let overlap = title_bar.intersect(&display);
    overlap.size.width >= token_size::TITLE_BAR && overlap.size.height >= token_size::TITLE_BAR
}

/// The view to open: the saved one, or [`View::default`] when there's none
/// or the key isn't a view this build has.
pub fn restored_view(saved: Option<&WindowState>) -> View {
    saved
        .and_then(|state| state.view.as_deref())
        .and_then(View::from_key)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    /// A 2560×1440 primary display with a 1920×1080 one to its left.
    fn displays() -> [Bounds<Pixels>; 2] {
        [
            Bounds::new(point(px(0.), px(0.)), size(px(2560.), px(1440.))),
            Bounds::new(point(px(-1920.), px(0.)), size(px(1920.), px(1080.))),
        ]
    }

    fn fallback() -> Bounds<Pixels> {
        Bounds::new(point(px(560.), px(270.)), size(px(1440.), px(900.)))
    }

    fn saved(x: i32, y: i32, width: u32, height: u32) -> WindowState {
        WindowState {
            mode: WindowMode::Windowed,
            x,
            y,
            width,
            height,
            view: Some("transactions".into()),
        }
    }

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    #[test]
    fn without_a_saved_state_the_window_opens_at_the_fallback() {
        assert_eq!(
            restored_bounds(None, &displays(), fallback()),
            WindowBounds::Windowed(fallback())
        );
    }

    #[test]
    fn a_window_on_a_display_reopens_where_it_was() {
        let state = saved(-1800, 40, 1600, 1000);
        assert_eq!(
            restored_bounds(Some(&state), &displays(), fallback()),
            WindowBounds::Windowed(bounds(-1800., 40., 1600., 1000.)),
            "on the second display"
        );
    }

    #[test]
    fn a_window_off_every_display_is_centred_instead() {
        // The display it was on has been unplugged.
        let gone = saved(-3600, 100, 1440, 900);
        assert_eq!(
            restored_bounds(Some(&gone), &displays(), fallback()),
            WindowBounds::Windowed(fallback())
        );
        // Below the bottom edge, title bar and all.
        let below = saved(100, 1430, 1440, 900);
        assert_eq!(
            restored_bounds(Some(&below), &displays(), fallback()),
            WindowBounds::Windowed(fallback())
        );
    }

    #[test]
    fn a_title_bar_partly_on_screen_is_enough() {
        // Hanging off the right edge, with 100px of title bar still showing.
        let state = saved(2460, 300, 1440, 900);
        assert_eq!(
            restored_bounds(Some(&state), &displays(), fallback()),
            WindowBounds::Windowed(bounds(2460., 300., 1440., 900.))
        );
        // 10px showing isn't enough to grab.
        let sliver = saved(2550, 300, 1440, 900);
        assert_eq!(
            restored_bounds(Some(&sliver), &displays(), fallback()),
            WindowBounds::Windowed(fallback())
        );
    }

    #[test]
    fn a_small_window_grows_to_the_minimum() {
        let state = saved(100, 100, 800, 600);
        assert_eq!(
            restored_bounds(Some(&state), &displays(), fallback()),
            WindowBounds::Windowed(bounds(100., 100., 1280., 800.))
        );
    }

    #[test]
    fn maximised_and_full_screen_windows_keep_their_mode() {
        let maximised = WindowState {
            mode: WindowMode::Maximized,
            ..saved(0, 0, 1440, 900)
        };
        assert_eq!(
            restored_bounds(Some(&maximised), &displays(), fallback()),
            WindowBounds::Maximized(bounds(0., 0., 1440., 900.))
        );
        let full = WindowState {
            mode: WindowMode::Fullscreen,
            ..saved(0, 0, 1440, 900)
        };
        assert_eq!(
            restored_bounds(Some(&full), &displays(), fallback()),
            WindowBounds::Fullscreen(bounds(0., 0., 1440., 900.))
        );
    }

    #[test]
    fn without_displays_the_saved_bounds_are_trusted() {
        let state = saved(-3600, 100, 1440, 900);
        assert_eq!(
            restored_bounds(Some(&state), &[], fallback()),
            WindowBounds::Windowed(bounds(-3600., 100., 1440., 900.))
        );
    }

    #[test]
    fn a_snapshot_rounds_and_records_the_view() {
        let state = snapshot(
            WindowBounds::Maximized(bounds(-0.4, 24.6, 1440.5, 899.2)),
            View::Rules,
        );
        assert_eq!(
            state,
            WindowState {
                mode: WindowMode::Maximized,
                x: 0,
                y: 25,
                width: 1441,
                height: 899,
                view: Some("rules".into()),
            }
        );
        // Out-of-range values are clamped, so a save is never refused.
        let huge = snapshot(
            WindowBounds::Windowed(bounds(-1e9, 1e9, 1e9, 0.)),
            View::Overview,
        );
        let extent = WindowState::MAX_EXTENT;
        assert_eq!(
            (huge.x, huge.y, huge.width, huge.height),
            (-(extent as i32), extent as i32, extent, 1)
        );
    }

    #[test]
    fn the_view_is_restored_when_this_build_has_it() {
        assert_eq!(restored_view(None), View::default());
        assert_eq!(
            restored_view(Some(&WindowState {
                view: Some("net_worth".into()),
                ..saved(0, 0, 1440, 900)
            })),
            View::NetWorth
        );
        assert_eq!(
            restored_view(Some(&WindowState {
                view: Some("forecast".into()),
                ..saved(0, 0, 1440, 900)
            })),
            View::default(),
            "a view from a newer build"
        );
    }

    #[test]
    fn the_store_saves_and_loads_through_the_database() {
        let dir = TempDir::new().unwrap();
        let store = WindowStore::new(dir.path().join("coffer.db"));
        assert_eq!(store.load(), None, "a new database has none");
        let state = snapshot(
            WindowBounds::Windowed(bounds(10., 20., 1500., 950.)),
            View::Budgets,
        );
        store.save(&state);
        assert_eq!(store.load(), Some(state));
    }

    #[test]
    fn the_store_waits_out_a_restore() {
        let dir = TempDir::new().unwrap();
        let database = dir.path().join("coffer.db");
        let store = WindowStore::new(database.clone());
        let state = snapshot(WindowBounds::Windowed(fallback()), View::Inbox);
        store.save(&state);

        // A restore holds an exclusive lock on `<database>-restoring`.
        let lock = std::fs::File::create(backup::restore_lock_path(&database)).unwrap();
        lock.lock().unwrap();
        assert_eq!(store.load(), None, "nothing is read during a restore");
        store.save(&snapshot(WindowBounds::Windowed(fallback()), View::Rules));
        lock.unlock().unwrap();

        assert_eq!(store.load(), Some(state), "nothing was written either");
    }
}
