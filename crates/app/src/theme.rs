//! Coffer's visual theme. See `docs/DESIGN.md`.
//!
//! Lives in the root crate until the M1 workspace split, then moves to
//! `crates/app`.

pub mod component;
pub mod focus;
pub mod fonts;
pub mod tokens;

use gpui_kit::App;

/// Installs the whole theme: `gpui-component`'s theme mapped onto the
/// tokens, then the bundled fonts.
///
/// Call once at startup, after `gpui_kit::init` and before opening a window.
///
/// # Errors
///
/// [`fonts::FontError`] when the bundled fonts don't load; see
/// [`fonts::install`].
pub fn install(cx: &mut App) -> Result<(), fonts::FontError> {
    component::install(cx);
    fonts::install(cx)
}
