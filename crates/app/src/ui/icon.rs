//! Coffer's icons (`docs/DESIGN.md` → "Iconography").
//!
//! The paths are copied from the Paper Components artboard, so they keep its
//! outline style: 1.2–1.3px strokes with round caps and joins, drawn at the
//! size of their `viewBox`. Only the agent's sparkle and Coffer's mark are
//! filled. GPUI draws an
//! SVG as a mask in the element's text colour, so the colour inside the
//! markup is irrelevant (`currentColor`) and the caller sets it with
//! `text_color`.
//!
//! The icons are `svg()` elements rather than stock `Icon`s because a stock
//! icon fixes its colour when it renders, so a parent's hover style can't
//! change it. An `svg()` takes `group_hover`, which is how hovered rows and
//! icon buttons brighten their icon.

use gpui_kit::{Pixels, Styled as _, Svg, svg};

use crate::theme::tokens::size;

/// One of Coffer's icons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Glyph {
    /// A bank account (card outline). 12px.
    Account,
    /// A savings pot under its parent account. 12px.
    Pot,
    /// Add. 12px.
    Plus,
    /// A transfer between own accounts, including pot moves (⇄). 12px.
    Transfer,
    /// The AI agent, and nothing else. Filled. 12px.
    Sparkle,
    /// Approve. 12px.
    Check,
    /// Reject or dismiss. 10px.
    Close,
    /// Sort direction and select chevron. 8px.
    ChevronDown,
    /// Show or hide a side panel. 16px.
    Panel,
    /// Search fields and the command bar. 12px.
    Search,
    /// Minimise the window. 10px.
    Minimize,
    /// Maximise or restore the window. 10px.
    Maximize,
    /// Settings (sliders), in the title bar. 14px.
    Settings,
    /// Coffer's mark, the strongbox (`DESIGN.md` → "Logo"). Filled, drawn
    /// in `molten` beside the wordmark. 16px.
    Mark,
    /// A cash account: a banknote (Paper `FHZ-0`, the Kind menu). 12px.
    Cash,
    /// An asset account: an isometric box, something you own (D6.9). 12px.
    Asset,
    /// A liability account: a receipt with a minus (D6.9). 12px.
    Liability,
    /// A value that can't change here, after it in a read-only field
    /// (the inspector's Currency, Paper `8GJ-0`). 10px.
    Lock,
    /// The Inbox: a tray (Paper `564-0`, the Pending row; `EAF-0`, a write
    /// to the Inbox in the access log). 12px.
    Inbox,
    /// Rejected, in the Inbox's status list: a 12px cross, where
    /// [`Glyph::Close`] is the 10px dismiss. 12px.
    Reject,
    /// A merchant name (an alias): a name tag (D6.7). 12px.
    NameTag,
    /// A refund, reimbursement or settlement link (D6.5). 12px.
    Link,
    /// A categorisation: an arrow to the category (Paper `564-0`). 12px.
    Arrow,
    /// A value the user set, which a suggestion leaves alone ("Stays, you
    /// set it", D6.6): a person. 10px.
    YouSet,
    /// A warning: a triangle with an exclamation mark (D6.4, D6.6's wide
    /// rule). 14px.
    Warning,
    /// The chosen item in a menu (Paper `FHZ-0`). Its tick is a little
    /// shorter than [`Glyph::Check`]'s. 12px.
    Chosen,
    /// AI clients in Settings: a plug (Paper `BPW-0`). 12px.
    Plug,
    /// The access log in Settings: a list (Paper `BQ2-0`). 12px.
    Log,
    /// Backups in Settings: a clock turning back (Paper `BQY-0`). 12px.
    Backup,
    /// Export…: an arrow out of a tray (Paper `BR4-0`). 12px.
    Export,
    /// Refused: a circle with a slash (Paper `E9T-0`): a refused call, AI
    /// access off. 12px; the off banner draws it at 14px.
    Refused,
    /// Copy to the clipboard (Paper `D9W-0`). 12px.
    Copy,
    /// A check that failed: a circle with an exclamation mark (Paper
    /// `H8G-0`). 12px.
    Alert,
    /// Dismiss a notification (Paper `HF9-0`). 12px.
    Dismiss,
    /// The Import drop zone's well: an arrow down onto a line (Paper
    /// `6JV-0`). 14px.
    Download,
    /// Information: a circle with an i, the import's "Not reconciled"
    /// (Paper `6N3-0`). 14px.
    Info,
    /// The import's "Reconciled": a 14px check (Paper `6NI-0`).
    Reconciled,
    /// The import's "Balances don't match": a circle with an exclamation
    /// mark (Paper `6NP-0`). 14px.
    Mismatch,
    /// The import's locked period: a 14px lock (Paper `6NX-0`).
    LockedPeriod,
}

impl Glyph {
    /// The size the icon is drawn at: its `viewBox`, so strokes stay at the
    /// width Paper draws them.
    pub fn size(self) -> Pixels {
        match self {
            Self::Account
            | Self::Pot
            | Self::Plus
            | Self::Transfer
            | Self::Sparkle
            | Self::Check
            | Self::Search
            | Self::Cash
            | Self::Asset
            | Self::Liability
            | Self::Chosen
            | Self::Inbox
            | Self::Reject
            | Self::NameTag
            | Self::Link
            | Self::Arrow
            | Self::Plug
            | Self::Log
            | Self::Backup
            | Self::Export
            | Self::Refused
            | Self::Copy
            | Self::Alert
            | Self::Dismiss => size::ICON_SM,
            Self::Close | Self::Minimize | Self::Maximize | Self::Lock | Self::YouSet => {
                size::ICON_XS
            }
            Self::Settings
            | Self::Warning
            | Self::Download
            | Self::Info
            | Self::Reconciled
            | Self::Mismatch
            | Self::LockedPeriod => size::ICON_MD,
            Self::ChevronDown => size::MARKER,
            Self::Panel | Self::Mark => size::ICON_LG,
        }
    }

    /// The icon as SVG markup.
    pub fn markup(self) -> &'static str {
        match self {
            Self::Account => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<rect x="1.5" y="2.75" width="9" height="6.5" rx="1.25"/><path d="M1.5 5.25H10.5"/></svg>"#
            ),
            Self::Pot => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M2.75 4.5H9.25V8.75A2 2 0 0 1 7.25 10.75H4.75A2 2 0 0 1 2.75 8.75Z"/><path d="M4 2.25H8"/></svg>"#
            ),
            Self::Plus => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M6 2.5V9.5M2.5 6H9.5"/></svg>"#
            ),
            Self::Transfer => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M2 4.25H9.5M7.75 2.5L9.5 4.25L7.75 6M10 7.75H2.5M4.25 6L2.5 7.75L4.25 9.5"/></svg>"#
            ),
            Self::Sparkle => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12">"#,
                r#"<path d="M6 0.5L7.3 4.7L11.5 6L7.3 7.3L6 11.5L4.7 7.3L0.5 6L4.7 4.7Z" fill="currentColor"/></svg>"#
            ),
            Self::Check => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M2.5 6.25L5 8.75L9.5 3.75"/></svg>"#
            ),
            Self::Close => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M2.5 2.5L7.5 7.5M7.5 2.5L2.5 7.5"/></svg>"#
            ),
            Self::ChevronDown => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M1.5 2.75L4 5.25L6.5 2.75"/></svg>"#
            ),
            Self::Panel => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<rect x="2.25" y="3" width="11.5" height="10" rx="2"/><path d="M6.25 3V13"/></svg>"#
            ),
            Self::Search => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<circle cx="5.25" cy="5.25" r="3.75"/><path d="M8 8L10.5 10.5"/></svg>"#
            ),
            Self::Minimize => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M2 5H8"/></svg>"#
            ),
            Self::Maximize => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<rect x="2" y="2" width="6" height="6" rx="1"/></svg>"#
            ),
            Self::Settings => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M2 4H6.5M9.5 4H12M2 10H4.5M7.5 10H12"/><circle cx="8" cy="4" r="1.5"/><circle cx="6" cy="10" r="1.5"/></svg>"#
            ),
            Self::Mark => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="currentColor">"#,
                r#"<path d="M2.5 2H13.5A1.5 1.5 0 0 1 15 3.5V6H1V3.5A1.5 1.5 0 0 1 2.5 2Z"/>"#,
                r#"<path d="M1 7H7V9A1 1 0 0 0 9 9V7H15V12.5A1.5 1.5 0 0 1 13.5 14H2.5A1.5 1.5 0 0 1 1 12.5Z"/></svg>"#
            ),
            Self::Cash => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M1.5 3.5H10.5V8.5H1.5ZM6 7.25A1.25 1.25 0 1 0 6 4.75A1.25 1.25 0 0 0 6 7.25Z"/></svg>"#
            ),
            Self::Asset => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M6 1.5L10.25 3.75V8.25L6 10.5L1.75 8.25V3.75Z"/><path d="M1.75 3.75L6 6L10.25 3.75M6 6V10.5"/></svg>"#
            ),
            Self::Liability => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M2.75 1.5H9.25V10.5L7.625 9.5L6 10.5L4.375 9.5L2.75 10.5Z"/><path d="M4.5 5.25H7.5"/></svg>"#
            ),
            Self::Lock => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.1">"#,
                r#"<rect x="2" y="4.5" width="6" height="4" rx="1"/><path d="M3.25 4.5V3.25C3.25 2.3 4 1.5 5 1.5C6 1.5 6.75 2.3 6.75 3.25V4.5"/></svg>"#
            ),
            Self::Inbox => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M1.5 6.75H4L4.75 8H7.25L8 6.75H10.5M1.5 6.75L3 2.75H9L10.5 6.75V9.25H1.5Z"/></svg>"#
            ),
            Self::Reject => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M3 3L9 9M9 3L3 9"/></svg>"#
            ),
            Self::NameTag => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round">"#,
                r#"<path d="M1.75 2.75H6.5L10.25 6L6.5 9.25H1.75Z"/></svg>"#
            ),
            Self::Link => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M5 6.5A2.5 2.5 0 0 0 8.77 6.77L10.27 5.27A2.5 2.5 0 0 0 6.73 1.73L5.87 2.59M7 5.5A2.5 2.5 0 0 0 3.23 5.23L1.73 6.73A2.5 2.5 0 0 0 5.27 10.27L6.12 9.42"/></svg>"#
            ),
            Self::Arrow => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M2 6H9.5M7 3.5L9.5 6L7 8.5"/></svg>"#
            ),
            Self::YouSet => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.1" stroke-linecap="round">"#,
                r#"<circle cx="5" cy="3.25" r="1.75"/><path d="M1.75 9C1.75 7.2 3.2 6.25 5 6.25C6.8 6.25 8.25 7.2 8.25 9"/></svg>"#
            ),
            // Paper fills the exclamation's dot (r 0.65); a round cap 1.3
            // wide on a zero-length stroke draws the same dot and keeps
            // the icon an outline.
            Self::Warning => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M7 2.25L12.25 11.25H1.75Z"/><path d="M7 5.75V8"/><path d="M7 9.6L7 9.6" stroke-width="1.3"/></svg>"#
            ),
            Self::Chosen => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M2.5 6.25L4.75 8.5L9.5 3.5"/></svg>"#
            ),
            Self::Plug => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M4.5 1.5V3.5M7.5 1.5V3.5M3 3.5H9V5.5A3 3 0 0 1 3 5.5ZM6 8.5V10.5"/></svg>"#
            ),
            Self::Log => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M4.5 3H10M4.5 6H10M4.5 9H10M2 3H2.5M2 6H2.5M2 9H2.5"/></svg>"#
            ),
            Self::Backup => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M2 6A4 4 0 1 0 3.2 3.2M2 1.5V3.5H4M6 4V6L7.5 7"/></svg>"#
            ),
            Self::Export => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M6 7.5V1.5M3.75 3.75L6 1.5L8.25 3.75M2 7V10H10V7"/></svg>"#
            ),
            Self::Refused => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<circle cx="6" cy="6" r="4.25"/><path d="M3 9L9 3"/></svg>"#
            ),
            Self::Copy => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<rect x="4" y="4" width="6.5" height="6.5" rx="1.25"/><path d="M8 2.5V2.25A0.75 0.75 0 0 0 7.25 1.5H2.25A0.75 0.75 0 0 0 1.5 2.25V7.25A0.75 0.75 0 0 0 2.25 8H2.5"/></svg>"#
            ),
            Self::Alert => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<circle cx="6" cy="6" r="4.75"/><path d="M6 3.5V6.25M6 8.25V8.3"/></svg>"#
            ),
            Self::Dismiss => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M3 3L9 9M9 3L3 9"/></svg>"#
            ),
            Self::Download => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M7 2V9M4 6L7 9L10 6M2.5 11.5H11.5"/></svg>"#
            ),
            Self::Info => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round">"#,
                r#"<circle cx="7" cy="7" r="5.5"/><path d="M7 6.25V9.75M7 4.25V4.3"/></svg>"#
            ),
            Self::Reconciled => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">"#,
                r#"<path d="M3 7.25L6 10.25L11 4.25"/></svg>"#
            ),
            Self::Mismatch => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round">"#,
                r#"<circle cx="7" cy="7" r="5.5"/><path d="M7 4.25V7.75M7 9.7V9.75"/></svg>"#
            ),
            Self::LockedPeriod => concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round">"#,
                r#"<rect x="3" y="6.25" width="8" height="5.25" rx="1.25"/><path d="M4.75 6.25V4.75C4.75 3.5 5.75 2.5 7 2.5C8.25 2.5 9.25 3.5 9.25 4.75V6.25"/></svg>"#
            ),
        }
    }

    /// The icon as an element at its own size, in the inherited text
    /// colour until the caller sets one.
    pub fn render(self) -> Svg {
        self.render_at(self.size())
    }

    /// The icon scaled to `side`, strokes included. Only where a spec draws
    /// an icon off its own size, such as the 10px sparkle before an
    /// AI-suggested category.
    pub fn render_at(self, side: Pixels) -> Svg {
        svg()
            .data(self.markup().as_bytes())
            .flex_none()
            .w(side)
            .h(side)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::px;

    use super::*;

    const ALL: [Glyph; 39] = [
        Glyph::Account,
        Glyph::Pot,
        Glyph::Plus,
        Glyph::Transfer,
        Glyph::Sparkle,
        Glyph::Check,
        Glyph::Close,
        Glyph::ChevronDown,
        Glyph::Panel,
        Glyph::Search,
        Glyph::Minimize,
        Glyph::Maximize,
        Glyph::Settings,
        Glyph::Mark,
        Glyph::Cash,
        Glyph::Asset,
        Glyph::Liability,
        Glyph::Lock,
        Glyph::Chosen,
        Glyph::Inbox,
        Glyph::Reject,
        Glyph::NameTag,
        Glyph::Link,
        Glyph::Arrow,
        Glyph::YouSet,
        Glyph::Warning,
        Glyph::Plug,
        Glyph::Log,
        Glyph::Backup,
        Glyph::Export,
        Glyph::Refused,
        Glyph::Copy,
        Glyph::Alert,
        Glyph::Dismiss,
        Glyph::Download,
        Glyph::Info,
        Glyph::Reconciled,
        Glyph::Mismatch,
        Glyph::LockedPeriod,
    ];

    /// The `viewBox` side in the markup, which `size` must match so strokes
    /// render at their designed width.
    fn view_box_side(markup: &str) -> f32 {
        let start = markup.find("viewBox=\"").expect("has a viewBox") + "viewBox=\"".len();
        let value = &markup[start..start + markup[start..].find('"').expect("closed")];
        let parts: Vec<f32> = value
            .split(' ')
            .map(|n| n.parse().expect("number"))
            .collect();
        assert_eq!(parts[..2], [0., 0.], "viewBox starts at the origin");
        assert_eq!(parts[2], parts[3], "icons are square");
        parts[2]
    }

    #[test]
    fn every_glyph_is_drawn_at_its_view_box() {
        for glyph in ALL {
            let markup = glyph.markup();
            assert_eq!(px(view_box_side(markup)), glyph.size(), "{glyph:?}");
        }
    }

    #[test]
    fn markup_carries_no_colour() {
        // The element's text colour paints the icon; a literal colour in the
        // markup would be a colour outside the theme.
        for glyph in ALL {
            let markup = glyph.markup();
            assert!(!markup.contains('#'), "{glyph:?}");
            for attribute in ["stroke=\"", "fill=\""] {
                for value in markup.split(attribute).skip(1) {
                    let value = &value[..value.find('"').expect("closed")];
                    assert!(
                        value == "currentColor" || value == "none",
                        "{glyph:?}: {attribute}{value}"
                    );
                }
            }
        }
    }

    #[test]
    fn only_the_sparkle_and_the_mark_are_filled() {
        for glyph in ALL {
            let filled = glyph.markup().contains("fill=\"currentColor\"");
            let solid = matches!(glyph, Glyph::Sparkle | Glyph::Mark);
            assert_eq!(filled, solid, "{glyph:?}");
        }
    }

    /// The `d` attributes of every path in `markup`, in order.
    fn path_data(markup: &str) -> Vec<&str> {
        markup
            .split(" d=\"")
            .skip(1)
            .map(|rest| &rest[..rest.find('"').expect("closed")])
            .collect()
    }

    #[test]
    fn the_mark_matches_the_brand_assets() {
        // The SVGs in `assets/brand/` are the mark outside the app (app icon,
        // README); they must draw the same strongbox as the title bar.
        let mark = path_data(Glyph::Mark.markup());
        assert_eq!(mark.len(), 2, "lid and body");
        for asset in [
            include_str!("../../../../assets/brand/coffer-mark.svg"),
            include_str!("../../../../assets/brand/coffer-icon.svg"),
        ] {
            assert_eq!(path_data(asset), mark);
        }
    }
}
