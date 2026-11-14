//! Settings' left panel (Paper `AYL-0`): AI ACCESS, with Clients (count of
//! clients) and Access log (calls in the last 7 days); DATA, with Backups
//! (count) and Export… (opens the export dialog, so it's never selected).
//! At the foot, above a `hairline`, where the data lives.
//!
//! A Tab stop: the arrow keys move between the three screens.

use gpui_kit::base::actions::{SelectDown, SelectUp};
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
    Window, div,
};

use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::icon::Glyph;
use crate::ui::list_row::{ListRow, TrailingTone};
use crate::ui::section_label::SectionLabelRow;
use crate::ui::text::TextRoleExt as _;

use super::table::count;
use super::{Section, SettingsView};

/// The panel's key context.
pub const CONTEXT: &str = "CofferSettingsPanel";

/// A list under a section label: 8px top, 10px bottom, 6px inline padding
/// (Paper `BPR-0`).
fn list_section(label: &'static str) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .pt(spacing::S2)
        .px(spacing::S1_5)
        .pb(spacing::S2_5)
        .child(SectionLabelRow::new(label))
}

impl SettingsView {
    fn section_row(
        &self,
        section: Section,
        glyph: Glyph,
        label: &'static str,
        trailing: String,
        ring: bool,
        cx: &mut Context<Self>,
    ) -> ListRow {
        let selected = self.section == section;
        let focus = self.panel_focus.clone();
        let id = match section {
            Section::Clients => "settings-clients",
            Section::Log => "settings-log",
            Section::Backups => "settings-backups",
        };
        ListRow::new(id, label)
            .icon(glyph)
            .selected(selected)
            .focus_visible(selected && ring)
            .trailing(trailing)
            // Paper `BPZ-0`: the selected row's count lifts to
            // `text-secondary`, which the row does for a muted value.
            .trailing_tone(if selected {
                TrailingTone::Muted
            } else {
                TrailingTone::Faint
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                focus.focus(window, cx);
                this.show(section, cx);
            }))
    }

    pub(super) fn render_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ring = self.panel_focus.is_focused(window) && window.last_input_was_keyboard();
        let ai = list_section("AI access")
            .child(self.section_row(
                Section::Clients,
                Glyph::Plug,
                "Clients",
                self.ai.clients.len().to_string(),
                ring,
                cx,
            ))
            .child(self.section_row(
                Section::Log,
                Glyph::Log,
                "Access log",
                count(self.ai.calls_week),
                ring,
                cx,
            ));
        let data = list_section("Data")
            .child(self.section_row(
                Section::Backups,
                Glyph::Backup,
                "Backups",
                self.backups.rows.len().to_string(),
                ring,
                cx,
            ))
            .child(
                ListRow::new("settings-export", "Export…")
                    .icon(Glyph::Export)
                    .on_click(cx.listener(|this, _, _, cx| this.request_export(cx))),
            );
        let foot = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(spacing::S1)
            .p(spacing::S3_5)
            .border_t(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .child(
                div()
                    .text_role(typography::CAPTION)
                    .text_color(color::TEXT_MUTED)
                    .child("Everything is in one file on this computer"),
            )
            .child(
                div()
                    .text_role(typography::MONO_VALUE)
                    .text_color(color::TEXT_SECONDARY)
                    .child(self.data_file.clone()),
            );
        div()
            .id("settings-panel")
            .key_context(CONTEXT)
            .track_focus(&self.panel_focus.clone().tab_stop(true))
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.show_next(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.show_next(1, cx)))
            .flex()
            .flex_col()
            .flex_none()
            .w(size::PANEL)
            .overflow_hidden()
            .bg(color::CHROME)
            .border_r(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .child(ai)
            .child(data)
            .child(div().flex_1())
            .child(foot)
            .into_any_element()
    }
}
