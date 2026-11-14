//! List rows (`docs/DESIGN.md` → "Components" → **List row**): the accounts
//! and categories in the left panel.
//!
//! [`ListRow`] wraps the stock `ListItem`, which keeps the click handling and
//! the hover registration a stock `List` or `Tree` expects of its items. The
//! row's look comes from the tokens: 28px, radius 6, 8px inline padding, an
//! icon in a 16px slot, a 12/16 500 label and a trailing mono value.
//!
//! A row doesn't own keyboard focus. In a list, the list holds focus and the
//! arrow keys move its current row, so the list tells the row when to draw
//! the focus ring ([`ListRow::focus_visible`]). The ring is inset, because a
//! scrolling list clips anything outside its rows, and it's drawn on a
//! wrapper: the stock item puts children in an inner box, not its own.

use gpui_kit::component::list::ListItem;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, ElementId, HighlightStyle, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, Rgba, SharedString, Styled as _, StyledText, Window, div,
};

use crate::theme::focus::{FocusRingExt as _, FocusRingPlacement};
use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, radius, size, spacing, typography, weight};
use crate::ui::PreviewState;
use crate::ui::icon::Glyph;
use crate::ui::marker::Marker;
use crate::ui::text::TextRoleExt as _;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The `group` name on a row, for content that follows its hover.
const GROUP: &str = "coffer-list-row";

/// The row label: body type at medium weight.
const LABEL: TextRole = TextRole {
    weight: weight::MEDIUM,
    ..typography::BODY
};

/// What sits in a row's 16px leading slot.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Leading {
    Icon(Glyph),
    Marker(Marker),
    /// A 6px dot: a connected agent (`moldavite`).
    Dot(Rgba),
    /// Nothing, but the slot is kept so the label lines up with its
    /// neighbours'.
    Blank,
}

/// The colour of a row's trailing value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrailingTone {
    /// `text-muted`: balances.
    #[default]
    Muted,
    /// `text-faint`: counts, which matter less than balances.
    Faint,
    /// `danger`: a negative balance (`DESIGN.md` → "Money").
    Danger,
}

impl TrailingTone {
    fn color(self) -> Rgba {
        match self {
            Self::Muted => color::TEXT_MUTED,
            Self::Faint => color::TEXT_FAINT,
            Self::Danger => color::DANGER,
        }
    }
}

/// A row in a list: icon, label and a trailing value.
///
/// Stateless; the owner decides which row is selected and which is the
/// keyboard target, and what a click does. The label is the row's accessible
/// name. The row is as wide as its column.
///
/// ```
/// use coffer_app::ui::icon::Glyph;
/// use coffer_app::ui::list_row::ListRow;
///
/// let row = ListRow::new("account-current", "Current account")
///     .icon(Glyph::Account)
///     .trailing("£1,284.10")
///     .selected(true);
/// assert!(row.is_selected());
/// ```
#[derive(IntoElement)]
pub struct ListRow {
    id: ElementId,
    label: SharedString,
    leading: Option<Leading>,
    /// Words after the label at regular weight in `text-muted`: "Coffer
    /// from your edits".
    label_suffix: Option<SharedString>,
    trailing: Option<SharedString>,
    /// The trailing value is words ("No balance"), set in UI type.
    trailing_words: bool,
    trailing_tone: TrailingTone,
    flagged: bool,
    dimmed: bool,
    nested: bool,
    selected: bool,
    focus_visible: bool,
    preview: Option<PreviewState>,
    on_click: Option<ClickHandler>,
}

impl ListRow {
    /// An unselected row showing `label`.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            leading: None,
            label_suffix: None,
            trailing: None,
            trailing_words: false,
            trailing_tone: TrailingTone::Muted,
            flagged: false,
            dimmed: false,
            nested: false,
            selected: false,
            focus_visible: false,
            preview: None,
            on_click: None,
        }
    }

    /// Shows `glyph` before the label, in `text-muted`; `text` while the row
    /// is hovered or selected.
    pub fn icon(mut self, glyph: Glyph) -> Self {
        self.leading = Some(Leading::Icon(glyph));
        self
    }

    /// Shows a category marker before the label, in the icon's slot.
    pub fn marker(mut self, marker: Marker) -> Self {
        self.leading = Some(Leading::Marker(marker));
        self
    }

    /// Shows a 6px dot of `color` in the icon's slot: the source list's
    /// connected agent (Paper `564-0`).
    pub fn dot(mut self, color: Rgba) -> Self {
        self.leading = Some(Leading::Dot(color));
        self
    }

    /// Keeps the icon's slot empty, so the label lines up with rows that
    /// have one.
    pub fn blank_slot(mut self) -> Self {
        self.leading = Some(Leading::Blank);
        self
    }

    /// Adds `words` after the label at regular weight in `text-muted`
    /// ("Coffer" + "from your edits").
    pub fn label_suffix(mut self, words: impl Into<SharedString>) -> Self {
        self.label_suffix = Some(words.into());
        self
    }

    /// Sets the trailing value's colour; `text-muted` by default.
    pub fn trailing_tone(mut self, tone: TrailingTone) -> Self {
        self.trailing_tone = tone;
        self
    }

    /// Rests the label at `text-muted` instead of `text-secondary`, for a
    /// row that stands for an absence ("Uncategorised"). Hover and selection
    /// still lift it.
    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }

    /// Shows `value` at the end of the row in Geist Mono 11/14 `text-muted`:
    /// a balance or a count, formatted for the user's locale.
    pub fn trailing(mut self, value: impl Into<SharedString>) -> Self {
        self.trailing = Some(value.into());
        self
    }

    /// Shows `words` at the end of the row in caption type (Inter 11/14),
    /// where a value is missing: "No balance".
    pub fn trailing_words(mut self, words: impl Into<SharedString>) -> Self {
        self.trailing = Some(words.into());
        self.trailing_words = true;
        self
    }

    /// Puts a 6px `danger` dot before the trailing value: something about
    /// the row needs attention, such as a failed balance check.
    pub fn flagged(mut self, flagged: bool) -> Self {
        self.flagged = flagged;
        self
    }

    /// Indents the row 20px under its parent, as a pot under its account.
    pub fn nested(mut self, nested: bool) -> Self {
        self.nested = nested;
        self
    }

    /// Marks the row selected: `molten-soft` fill and a `white` label.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Draws the inset focus ring. The owning list sets it on its current
    /// row while the list has keyboard focus (focused, and the last input was
    /// a key press).
    pub fn focus_visible(mut self, focus_visible: bool) -> Self {
        self.focus_visible = focus_visible;
        self
    }

    /// Runs `handler` when the row is clicked.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Draws `state` without the input that causes it; see [`PreviewState`].
    /// Rows have no pressed look, so [`PreviewState::Pressed`] draws the
    /// resting row.
    pub fn preview(mut self, state: PreviewState) -> Self {
        self.preview = Some(state);
        self
    }

    pub fn is_selected(&self) -> bool {
        self.selected
    }

    pub fn is_focus_visible(&self) -> bool {
        self.focus_visible
    }

    pub fn is_nested(&self) -> bool {
        self.nested
    }

    pub fn is_flagged(&self) -> bool {
        self.flagged
    }

    fn colors(&self) -> RowColors {
        let colors = RowColors::new(self.selected, self.preview == Some(PreviewState::Hovered));
        if self.dimmed && colors.follows_hover {
            RowColors {
                label: color::TEXT_MUTED,
                ..colors
            }
        } else {
            colors
        }
    }
}

/// What a row's state does to its colours.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RowColors {
    /// `None`: no fill of its own; the stock item draws the selected fill.
    fill: Option<Rgba>,
    label: Rgba,
    icon: Rgba,
    /// Whether label and icon brighten to `text` under the pointer.
    follows_hover: bool,
}

impl RowColors {
    fn new(selected: bool, previewed_hover: bool) -> Self {
        if selected {
            // The stock item draws `list_active` (`molten-soft`) and drops
            // its hover fill while selected.
            Self {
                fill: None,
                label: color::WHITE,
                icon: color::TEXT,
                follows_hover: false,
            }
        } else if previewed_hover {
            Self {
                fill: Some(color::HOVER),
                label: color::TEXT,
                icon: color::TEXT,
                follows_hover: false,
            }
        } else {
            Self {
                fill: None,
                label: color::TEXT_SECONDARY,
                icon: color::TEXT_MUTED,
                follows_hover: true,
            }
        }
    }
}

/// The label, with its suffix (if any) at regular weight in `text-muted`.
fn label_text(label: &SharedString, suffix: Option<&SharedString>) -> StyledText {
    let Some(suffix) = suffix else {
        return StyledText::new(label.clone());
    };
    let text = format!("{label} {suffix}");
    let range = label.len() + 1..text.len();
    StyledText::new(text).with_highlights([(
        range,
        HighlightStyle {
            color: Some(color::TEXT_MUTED.into()),
            font_weight: Some(weight::REGULAR),
            ..HighlightStyle::default()
        },
    )])
}

impl RenderOnce for ListRow {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let colors = self.colors();
        let ring = self.focus_visible || self.preview == Some(PreviewState::FocusVisible);
        let brighten = |follows: bool| {
            move |this: gpui_kit::Div| {
                this.when(follows, |this| {
                    this.group_hover(GROUP, |style| style.text_color(color::TEXT))
                })
            }
        };

        let content = div()
            .flex()
            .items_center()
            .gap(spacing::S2)
            .w_full()
            .min_w_0()
            .when_some(self.leading, |this, leading| {
                // The icon slot is 16px whatever the glyph, so labels align.
                let slot = div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(size::ICON_LG);
                this.child(match leading {
                    // An svg takes its colour from its own style, not
                    // its parent's text colour.
                    Leading::Icon(glyph) => slot.child(
                        glyph
                            .render()
                            .text_color(colors.icon)
                            .when(colors.follows_hover, |this| {
                                this.group_hover(GROUP, |style| style.text_color(color::TEXT))
                            }),
                    ),
                    Leading::Marker(marker) => slot.child(marker.render()),
                    Leading::Dot(fill) => slot.child(
                        div()
                            .flex_none()
                            .size(size::DOT)
                            .rounded(radius::LABEL)
                            .bg(fill),
                    ),
                    Leading::Blank => slot,
                })
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_role(LABEL)
                    .text_color(colors.label)
                    .map(brighten(colors.follows_hover))
                    .child(label_text(&self.label, self.label_suffix.as_ref())),
            )
            .when(self.flagged, |this| {
                this.child(
                    div()
                        .flex_none()
                        .size(size::DOT)
                        .rounded(radius::LABEL)
                        .bg(color::DANGER),
                )
            })
            .when_some(self.trailing, |this, value| {
                // A selected row lifts its balance a step, as its label.
                let tone = match self.trailing_tone {
                    TrailingTone::Muted if self.selected => color::TEXT_SECONDARY,
                    tone => tone.color(),
                };
                this.child(
                    div()
                        .flex_none()
                        .text_role(if self.trailing_words {
                            typography::CAPTION
                        } else {
                            typography::MONO_VALUE
                        })
                        .text_color(tone)
                        .child(value),
                )
            });

        let item = ListItem::new(self.id)
            .accessibility_label(self.label)
            .selected(self.selected)
            .group(GROUP)
            .h(size::CONTROL)
            .py_0()
            .px(spacing::S2)
            .gap_0()
            .rounded(radius::MD)
            .when_some(colors.fill, |this, fill| this.bg(fill))
            .when_some(self.on_click, |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .child(content);

        div()
            .flex()
            .rounded(radius::MD)
            .when(self.nested, |this| this.ml(spacing::S5))
            .child(item.flex_1().min_w_0())
            .when(ring, |this| {
                this.show_focus_ring(FocusRingPlacement::Inset, window)
            })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Bounds, Context, Pixels, Render, TestAppContext, VisualTestContext, px};

    use super::*;
    use crate::theme;
    use crate::theme::focus::RING_SELECTOR;

    #[test]
    fn colours_follow_the_state() {
        // Paper `VI-0`: default, hover/current, selected.
        let rest = RowColors::new(false, false);
        assert_eq!(rest.fill, None);
        assert_eq!(
            (rest.label, rest.icon),
            (color::TEXT_SECONDARY, color::TEXT_MUTED)
        );
        assert!(rest.follows_hover);

        let hovered = RowColors::new(false, true);
        assert_eq!(hovered.fill, Some(color::HOVER));
        assert_eq!((hovered.label, hovered.icon), (color::TEXT, color::TEXT));

        let selected = RowColors::new(true, false);
        assert_eq!((selected.label, selected.icon), (color::WHITE, color::TEXT));
        assert!(!selected.follows_hover, "a selected row doesn't react");
        assert_eq!(
            RowColors::new(true, true),
            selected,
            "selected wins over a previewed hover"
        );
    }

    #[test]
    fn label_is_body_at_medium() {
        assert_eq!(
            (LABEL.size, LABEL.line_height, LABEL.weight),
            (px(12.), px(16.), weight::MEDIUM)
        );
    }

    /// A column of rows, as a list renders them.
    struct Harness {
        rows: Box<dyn Fn() -> Vec<ListRow>>,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .w(size::PANEL)
                .p(spacing::S1_5)
                .children((self.rows)())
        }
    }

    fn open(
        cx: &mut TestAppContext,
        rows: impl Fn() -> Vec<ListRow> + 'static,
    ) -> &mut VisualTestContext {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::component::install(cx);
        });
        let (_, cx) = cx.add_window_view(|_, _| Harness {
            rows: Box::new(rows),
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx
    }

    fn bounds(cx: &mut VisualTestContext, id: &'static str) -> Bounds<Pixels> {
        cx.update(|window, _| window.find(id).bounds())
    }

    #[gpui_kit::test]
    fn rows_are_28px_and_pots_indent(cx: &mut TestAppContext) {
        let cx = open(cx, || {
            vec![
                ListRow::new("current", "Current account").icon(Glyph::Account),
                ListRow::new("holiday", "Holiday")
                    .icon(Glyph::Pot)
                    .nested(true),
            ]
        });

        let parent = bounds(cx, "current");
        let pot = bounds(cx, "holiday");
        assert_eq!(parent.size.height, size::CONTROL);
        assert_eq!(pot.size.height, size::CONTROL);
        assert_eq!(pot.left() - parent.left(), spacing::S5);
        assert_eq!(pot.right(), parent.right(), "the indent narrows the row");
        assert_eq!(
            cx.update(|window, _| window.find("holiday").label().map(str::to_owned)),
            Some("Holiday".to_owned())
        );
    }

    #[gpui_kit::test]
    fn click_reaches_the_owner(cx: &mut TestAppContext) {
        let clicks = Rc::new(Cell::new(0));
        let cx = open(cx, {
            let clicks = clicks.clone();
            move || {
                let clicks = clicks.clone();
                vec![
                    ListRow::new("current", "Current account")
                        .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
                ]
            }
        });

        cx.update(|window, cx| window.click("current", cx));
        assert_eq!(clicks.get(), 1);
    }

    #[gpui_kit::test]
    fn focus_visible_draws_the_inset_ring(cx: &mut TestAppContext) {
        let cx = open(cx, || {
            vec![
                ListRow::new("current", "Current account"),
                ListRow::new("joint", "Joint account").focus_visible(true),
            ]
        });

        let ring = cx
            .debug_bounds(RING_SELECTOR)
            .expect("the focused row draws the ring");
        assert_eq!(ring, bounds(cx, "joint"), "inset: on the row's own edge");
    }
}
