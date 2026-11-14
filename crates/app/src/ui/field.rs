//! Fields (`docs/DESIGN.md` → "Components" → **Inspector field row**,
//! **Select**; Paper `JY-0`, `KA-0`, `KK-0`): the label-and-control rows of
//! the inspector, and the search field of the table's toolbar.
//!
//! Text entry is the stock `Input`, styled per instance ([`text_input`]):
//! its dark-mode fill is the border colour at 30%, so no theme value gives
//! `field` with a `hairline` border. [`Field`] shows a value that isn't
//! typed in place (a date), and [`SelectField`] is a select's closed
//! state, the trigger for its menu.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, Rgba, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div,
};
use gpui_kit::{Div, Pixels};

use crate::theme::tokens::{color, font, leading, radius, size, spacing, text, typography};
use crate::ui::icon::Glyph;
use crate::ui::marker::Marker;
use crate::ui::text::TextRoleExt as _;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A label and its control: 28px, the label in a 64px `text-muted` column,
/// the control filling the rest.
#[derive(IntoElement)]
pub struct FieldRow {
    label: SharedString,
    control: AnyElement,
}

impl FieldRow {
    pub fn new(label: impl Into<SharedString>, control: impl IntoElement) -> Self {
        Self {
            label: label.into(),
            control: control.into_any_element(),
        }
    }
}

impl RenderOnce for FieldRow {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .w_full()
            .h(size::CONTROL)
            .child(
                div()
                    .flex_none()
                    .w(size::FIELD_LABEL)
                    .whitespace_nowrap()
                    .text_role(typography::BODY)
                    .text_color(color::TEXT_MUTED)
                    .child(self.label),
            )
            .child(div().flex().flex_1().min_w_0().child(self.control))
    }
}

/// The field surface: `field` fill, radius 6, 28px, filling its row.
fn surface() -> gpui_kit::Div {
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .h(size::CONTROL)
        .rounded(radius::MD)
        .bg(color::FIELD)
}

/// A value shown in a field and edited elsewhere (the date picker): 8px
/// inline padding, the value in `text`, in Geist Mono for dates and figures.
#[derive(IntoElement)]
pub struct Field {
    value: SharedString,
    mono: bool,
}

impl Field {
    pub fn new(value: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            mono: false,
        }
    }

    /// Sets the value in Geist Mono: a date, an amount or a count.
    pub fn mono(mut self, mono: bool) -> Self {
        self.mono = mono;
        self
    }
}

impl RenderOnce for Field {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        surface().gap(spacing::S1_5).px(spacing::S2).child(
            div()
                .min_w_0()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_role(if self.mono {
                    typography::MONO_AMOUNT
                } else {
                    typography::BODY
                })
                .text_color(color::TEXT)
                .child(self.value),
        )
    }
}

/// What leads a select's value.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SelectLead {
    Marker(Marker),
    /// The agent's suggestion: a 10px sparkle.
    Suggested,
}

/// A select, closed: the value and an 8px chevron in `text-muted`, 10px
/// left and 8px right padding.
///
/// A category's value leads with its marker; a value the agent suggested
/// leads with the sparkle and is in `moldavite-text`; a placeholder
/// ("Choose a category") is in `text-muted`. Clicking it opens the menu,
/// which the owner draws.
#[derive(IntoElement)]
pub struct SelectField {
    id: ElementId,
    value: SharedString,
    lead: Option<SelectLead>,
    hint: Option<SharedString>,
    placeholder: bool,
    mono: bool,
    on_click: Option<ClickHandler>,
}

impl SelectField {
    pub fn new(id: impl Into<ElementId>, value: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            value: value.into(),
            lead: None,
            hint: None,
            placeholder: false,
            mono: false,
            on_click: None,
        }
    }

    /// Sets the value in Geist Mono at body size: a date format
    /// (`DD/MM/YYYY`, the Import dialog's Format, Paper `5ZQ-0`).
    pub fn mono(mut self, mono: bool) -> Self {
        self.mono = mono;
        self
    }

    /// Leads the value with a category marker.
    pub fn marker(mut self, marker: Marker) -> Self {
        self.lead = Some(SelectLead::Marker(marker));
        self
    }

    /// Marks the value as the agent's suggestion, not yet approved.
    pub fn suggested(mut self) -> Self {
        self.lead = Some(SelectLead::Suggested);
        self
    }

    /// Follows the value with `hint` after a `·`, in `text-muted`
    /// ("Aggregate · totals only", Paper `GZI-0`).
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Shows the value as a placeholder: nothing is chosen yet.
    pub fn placeholder(mut self, placeholder: bool) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Runs `handler` when the select is clicked: open its menu.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    fn value_color(&self) -> Rgba {
        if self.placeholder {
            color::TEXT_MUTED
        } else if self.lead == Some(SelectLead::Suggested) {
            color::MOLDAVITE_TEXT
        } else {
            color::TEXT
        }
    }
}

impl RenderOnce for SelectField {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let value_color = self.value_color();
        // The sparkle is 2px wider than a marker, so a 2px tighter gap keeps
        // the value in the marker's lane, as in the table.
        let gap = if self.lead == Some(SelectLead::Suggested) {
            spacing::S1_5
        } else {
            spacing::S2
        };
        let value = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(gap)
            .when_some(self.lead, |this, lead| {
                this.child(match lead {
                    SelectLead::Marker(marker) => marker.render().into_any_element(),
                    SelectLead::Suggested => Glyph::Sparkle
                        .render_at(size::ICON_XS)
                        .text_color(color::MOLDAVITE)
                        .into_any_element(),
                })
            })
            .child(
                div()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_role(if self.mono {
                        typography::MONO_AMOUNT
                    } else {
                        typography::BODY
                    })
                    .text_color(value_color)
                    .child(self.value)
                    .when_some(self.hint, |this, hint| {
                        this.flex().child(
                            div()
                                .text_color(color::TEXT_MUTED)
                                .child(format!("\u{a0}· {hint}")),
                        )
                    }),
            );
        surface()
            .id(self.id)
            .gap(spacing::S1_5)
            .pl(spacing::S2_5)
            .pr(spacing::S2)
            .child(value)
            .child(Glyph::ChevronDown.render().text_color(color::TEXT_MUTED))
            .when_some(self.on_click, |this, handler| {
                this.on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}

/// The stock text input styled as a Coffer field: `field` fill, radius 6,
/// 28px, 8px inline padding, body type; placeholder in `text-muted`.
///
/// Chain more styles on the result: a `hairline` border and a leading icon
/// for the toolbar's search field ([`search_input`]).
pub fn text_input(state: &Entity<InputState>) -> Input {
    Input::new(state)
        .h(size::CONTROL)
        .px(spacing::S2)
        .gap(spacing::S1_5)
        .rounded(radius::MD)
        .bg(color::FIELD)
        .border_0()
        .font_family(font::UI)
        .text_size(text::SM)
        .line_height(leading::UI)
}

/// The table's search field: a [`text_input`] with a `hairline` border,
/// 10px inline padding and a search icon, `--container-search-field` wide.
pub fn search_input(state: &Entity<InputState>) -> Input {
    text_input(state)
        .prefix(Glyph::Search.render().text_color(color::TEXT_MUTED))
        .w(size::SEARCH_FIELD)
        .flex_none()
        .px(spacing::S2_5)
        .gap(spacing::S2)
        .border(size::HAIRLINE)
        .border_color(color::HAIRLINE)
}

/// An amount field (Paper `8JE-0`, New account → Balance): the currency's
/// symbol in mono `text-faint`, then the amount in Geist Mono. The
/// placeholder ("Optional") is in UI type, so the font follows the value.
pub fn amount_input(
    state: &Entity<InputState>,
    prefix: impl Into<SharedString>,
    cx: &App,
) -> Input {
    let typed = !state.read(cx).value().is_empty();
    text_input(state)
        .prefix(
            div()
                .text_role(typography::MONO_AMOUNT)
                .text_color(color::TEXT_FAINT)
                .child(prefix.into()),
        )
        .when(typed, |this| this.font_family(font::MONO))
}

/// A date field typed as text, `9 Nov 2026`: 96px (`--container-column-
/// amount`), Geist Mono. No picker is drawn yet (`DESIGN.md` → "Accounts
/// management").
pub fn date_input(state: &Entity<InputState>) -> Input {
    text_input(state)
        .flex_none()
        .w(size::COLUMN_AMOUNT)
        .font_family(font::MONO)
}

/// A value that can't be changed here, followed by a 10px `icon-faint`
/// lock (Paper `8GJ-0`, the inspector's Currency once the account holds
/// money): no field fill, 8px inline padding, the value in `text`.
#[derive(IntoElement)]
pub struct LockedValue {
    value: SharedString,
    mono: bool,
}

impl LockedValue {
    pub fn new(value: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            mono: false,
        }
    }

    /// Sets the value in Geist Mono: a currency code.
    pub fn mono(mut self, mono: bool) -> Self {
        self.mono = mono;
        self
    }
}

impl RenderOnce for LockedValue {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(spacing::S1_5)
            .px(spacing::S2)
            .child(
                div()
                    .text_role(if self.mono {
                        typography::MONO_AMOUNT
                    } else {
                        typography::BODY
                    })
                    .text_color(color::TEXT)
                    .child(self.value),
            )
            .child(Glyph::Lock.render().text_color(color::ICON_FAINT))
    }
}

/// A caption under a form saying what's wrong with it, in `danger`
/// (composed for M3.2: no field error state is drawn).
pub fn error_caption(text: impl Into<SharedString>) -> Div {
    div()
        .text_role(typography::CAPTION)
        .text_color(color::DANGER)
        .child(text.into())
}

/// A caption: 11/14 `text-muted`, wrapping.
pub fn caption(text: impl Into<SharedString>) -> Div {
    div()
        .text_role(typography::CAPTION)
        .text_color(color::TEXT_MUTED)
        .child(text.into())
}

/// The width of a date lane in a list of dated rows: 48px for `9 Nov`
/// (Paper `8GJ-0`), 56px when a row shows a year, `2 Oct 25` (D6.9).
pub fn date_lane(with_year: bool) -> Pixels {
    if with_year {
        spacing::S6 * 2. + spacing::S2
    } else {
        spacing::S6 * 2.
    }
}
