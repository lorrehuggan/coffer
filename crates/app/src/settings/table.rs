//! The pieces Settings' three screens share (Paper `AQ2-0`, `DC6-0`,
//! `F63-0`): the canvas toolbar, groups of 28px rows under a header row
//! with caps column labels, inspector field rows with a value, text mixing
//! Inter and Geist Mono, and the notification bar.

use std::path::PathBuf;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, Div, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Rgba, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, StyledText, TextRun, div, font,
};

use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, font as family, radius, size, spacing, typography, weight};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::notification::NotificationBar;
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

use super::data::BackupsData;
use super::{Notice, NoticeMessage, SettingsView};

/// The key context of a Settings list, where the arrow keys move the
/// selection.
pub const LIST_CONTEXT: &str = "CofferSettingsList";

/// A canvas title: 13/16 600, tight tracking.
const TOOLBAR_TITLE: TextRole = typography::TITLE;

/// A group's title ("Clients", "Today, Tue 10 Nov"): 11/14 600.
const GROUP_TITLE: TextRole = TextRole {
    weight: weight::STRONG,
    ..typography::CAPTION
};

/// The width of the lane before a client's name: the status dot's (16px).
pub const LEAD: Pixels = size::ICON_LG;

/// The ON column: two small controls and a gap (Paper `CC4-0`).
pub fn on_lane() -> Pixels {
    size::CONTROL_SM * 2. + spacing::S1
}

/// The canvas toolbar: 52px, 12px block and 16px inline padding, a
/// `hairline` below; the title, a caption taking the rest, then `trailing`.
pub fn toolbar(
    title: impl Into<SharedString>,
    caption: impl Into<SharedString>,
    trailing: impl IntoElement,
) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S2)
        .h(size::CONTROL + spacing::S6)
        .py(spacing::S3)
        .px(spacing::S4)
        .border_b(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .child(
            div()
                .flex()
                .flex_none()
                .text_color(color::TEXT)
                .child(Tracked::new(TOOLBAR_TITLE, title)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child(caption.into()),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(spacing::S2)
                .child(trailing),
        )
}

/// The canvas body under the toolbar: 8px inline and 16px bottom padding,
/// scrolling.
pub fn body(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(spacing::S2)
        .pb(spacing::S4)
}

/// A group of rows: 8px above, 12px below; a `hairline` above all but the
/// first.
pub fn group(divided: bool) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .pt(spacing::S2)
        .pb(spacing::S3)
        .when(divided, |this| {
            this.border_t(size::HAIRLINE).border_color(color::HAIRLINE)
        })
}

/// A header or data row: 28px, 8px inline padding, 12px between lanes.
pub fn row() -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S3)
        .h(size::CONTROL)
        .px(spacing::S2)
        .rounded(radius::MD)
}

/// A clickable data row: `hover` under the pointer, `molten-soft` when
/// selected.
pub fn selectable_row(id: impl Into<ElementId>, selected: bool) -> Stateful<Div> {
    row()
        .id(id)
        .when(selected, |this| this.bg(color::MOLTEN_SOFT))
        .when(!selected, |this| this.hover(|style| style.bg(color::HOVER)))
}

/// A group's title and its count, in a lane `width` wide (or flex).
pub fn group_title(
    title: impl Into<SharedString>,
    count: Option<String>,
    width: Option<Pixels>,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(spacing::S2)
        .map(|this| match width {
            Some(width) => this.flex_none().w(width),
            None => this.flex_1().min_w_0(),
        })
        .child(
            div()
                .whitespace_nowrap()
                .text_role(GROUP_TITLE)
                .text_color(color::TEXT_SECONDARY)
                .child(title.into()),
        )
        .when_some(count, |this, count| {
            this.child(
                div()
                    .text_role(typography::MONO_VALUE)
                    .text_color(color::TEXT_MUTED)
                    .child(count),
            )
        })
}

/// A column label: caps 11/14 600 `text-muted`, `width` wide (or flex),
/// right-aligned for figures.
pub fn column_label(label: &'static str, width: Option<Pixels>, end: bool) -> Div {
    div()
        .flex()
        .overflow_hidden()
        .text_color(color::TEXT_MUTED)
        .map(|this| match width {
            Some(width) => this.flex_none().w(width),
            None => this.flex_1().min_w_0(),
        })
        .when(end, |this| this.justify_end())
        .child(Tracked::new(typography::SECTION_LABEL, label))
}

/// A lane of a data row: `width` wide (or flex), one line, clipped with an
/// ellipsis.
pub fn cell(width: Option<Pixels>) -> Div {
    div()
        .flex()
        .items_center()
        .overflow_hidden()
        .whitespace_nowrap()
        .map(|this| match width {
            Some(width) => this.flex_none().w(width),
            None => this.flex_1().min_w_0(),
        })
}

/// One run of [`rich`] text.
#[derive(Clone, Debug)]
pub struct Run {
    text: String,
    mono: bool,
    color: Option<Rgba>,
    weight: Option<FontWeight>,
}

/// Plain text in the paragraph's colour.
pub fn plain(text: impl Into<String>) -> Run {
    Run {
        text: text.into(),
        mono: false,
        color: None,
        weight: None,
    }
}

/// Text in Geist Mono: a name, a number, a command.
pub fn mono(text: impl Into<String>) -> Run {
    Run {
        mono: true,
        ..plain(text)
    }
}

impl Run {
    pub fn color(mut self, color: Rgba) -> Self {
        self.color = Some(color);
        self
    }

    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = Some(weight);
        self
    }
}

/// A wrapping paragraph of `runs` in `role`'s size and line height, each run
/// in Inter or Geist Mono (`“Its configuration runs coffer mcp --client
/// my-agent.”`, Paper `CRE-0`). Runs share the paragraph's size: GPUI's
/// text runs carry a font and a colour, not a size.
pub fn rich(role: TextRole, base: Rgba, runs: Vec<Run>) -> Div {
    let text: String = runs.iter().map(|run| run.text.as_str()).collect();
    let text_runs = runs
        .iter()
        .filter(|run| !run.text.is_empty())
        .map(|run| {
            let mut face = font(if run.mono { family::MONO } else { role.family });
            face.weight = run.weight.unwrap_or(role.weight);
            TextRun {
                len: run.text.len(),
                font: face,
                color: run.color.unwrap_or(base).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }
        })
        .collect();
    div()
        .text_role(role)
        .text_color(base)
        .child(StyledText::new(SharedString::from(text)).with_runs(text_runs))
}

/// An inspector row: a 64px `text-muted` label, then `value` with 8px
/// inline padding (Paper `EYI-0`).
pub fn value_row(label: &'static str, value: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S2)
        .h(size::CONTROL)
        .child(
            div()
                .flex_none()
                .w(size::FIELD_LABEL)
                .text_role(typography::BODY)
                .text_color(color::TEXT_MUTED)
                .child(label),
        )
        // Not clipped: a long value runs into the end padding, as on Paper
        // (`FX5-0`); the inspector clips at its edge.
        .child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .px(spacing::S2)
                .child(value),
        )
}

/// An inspector section with 8px between its parts (Paper `D87-0`), 14px
/// padding and a `hairline` above.
pub fn section(title: Option<&'static str>) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(spacing::S2)
        .p(spacing::S3_5)
        .border_t(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .when_some(title, |this, title| {
            this.child(
                div()
                    .text_role(typography::SECTION_TITLE)
                    .text_color(color::TEXT)
                    .child(title),
            )
        })
}

/// The inspector: 296px, `chrome`, a `hairline` on its canvas side,
/// scrolling.
pub fn inspector(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .flex_none()
        .w(size::INSPECTOR)
        .overflow_y_scroll()
        .bg(color::CHROME)
        .border_l(size::HAIRLINE)
        .border_color(color::HAIRLINE)
}

/// The inspector header: the title (`title_role`: 13/16 600, Inter or Geist
/// Mono), the figure, and a caption (Paper `ARQ-0`, `DDU-0`).
pub fn inspector_header(title: AnyElement, figure: Option<String>, caption: AnyElement) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(spacing::S1)
        .p(spacing::S3_5)
        .child(
            div()
                .flex()
                .min_w_0()
                .overflow_hidden()
                .text_color(color::TEXT)
                .child(title),
        )
        .when_some(figure, |this, figure| {
            this.child(
                div()
                    .flex()
                    .text_color(color::TEXT)
                    .child(Tracked::new(typography::DISPLAY_FIGURE, figure)),
            )
        })
        .child(caption)
}

/// A title in Geist Mono 13/16 600 (a tool, a client's name).
pub fn mono_title(text: impl Into<SharedString>) -> AnyElement {
    div()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_role(TextRole {
            family: family::MONO,
            tracking: 0.,
            ..typography::TITLE
        })
        .child(text.into())
        .into_any_element()
}

/// A caption: 11/14 `text-muted`, wrapping.
pub fn caption(text: impl Into<SharedString>) -> Div {
    div()
        .text_role(typography::CAPTION)
        .text_color(color::TEXT_MUTED)
        .child(text.into())
}

/// A count as the app writes it: `1,906`.
pub fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// `noun`, plural when `n` isn't one: `review` for 1, `suggestions` for 12.
pub fn plural(n: u64, noun: &str) -> String {
    if n == 1 {
        noun.to_owned()
    } else {
        format!("{noun}s")
    }
}

/// A size as the Backups screen shows it: megabytes with one decimal
/// (`4.2 MB`, `0.1 MB`), gigabytes past 1,024 MB. Binary units, as `coffer
/// backup` counts them.
pub fn file_size(bytes: u64) -> String {
    const MB: u128 = 1024 * 1024;
    let bytes = u128::from(bytes);
    let (divisor, unit) = if bytes >= 1024 * MB {
        (1024 * MB, "GB")
    } else {
        (MB, "MB")
    };
    let tenths = (bytes * 10 + divisor / 2) / divisor;
    format!("{}.{} {unit}", tenths / 10, tenths % 10)
}

/// The backups in the order the screen lists them: Recent, then One a
/// month.
pub fn backup_order(backups: &BackupsData) -> Vec<PathBuf> {
    let (monthly, recent): (Vec<_>, Vec<_>) = backups.rows.iter().partition(|row| row.monthly);
    recent
        .into_iter()
        .chain(monthly)
        .map(|row| row.backup.path.clone())
        .collect()
}

/// The notification bar for `notice`.
pub fn notice_bar(notice: &Notice, cx: &mut Context<SettingsView>) -> NotificationBar {
    let failed = matches!(notice.message, NoticeMessage::Failed(_));
    let message: AnyElement = match &notice.message {
        NoticeMessage::BackedUp { transactions, size } => {
            let mut runs = vec![plain("Backed up ")];
            if let Some(transactions) = transactions {
                runs.push(mono(count(*transactions)));
                runs.push(plain(format!(" {}", plural(*transactions, "transaction"))));
            } else {
                runs.push(plain("the database"));
            }
            runs.push(plain(" · ").color(color::TEXT_MUTED));
            runs.push(mono(file_size(*size)).color(color::TEXT_MUTED));
            rich(typography::BODY, color::TEXT, runs).into_any_element()
        }
        NoticeMessage::Exported { rows, file } => rich(
            typography::BODY,
            color::TEXT,
            vec![
                plain("Exported "),
                mono(count(*rows as u64)),
                plain(format!(" {} to ", plural(*rows as u64, "row"))),
                mono(file.clone()),
            ],
        )
        .into_any_element(),
        NoticeMessage::Copied { file } => rich(
            typography::BODY,
            color::TEXT,
            vec![plain("Saved a copy as "), mono(file.clone())],
        )
        .into_any_element(),
        NoticeMessage::Restored { migrated, log_kept } => {
            let mut text = String::from("Restored the backup");
            if *migrated {
                text.push_str(" and updated it to this version");
            }
            match log_kept {
                Some(0) | None => {}
                Some(rows) => text.push_str(&format!(
                    " · kept {rows} newer access-log {}",
                    if *rows == 1 { "entry" } else { "entries" }
                )),
            }
            div().child(text).into_any_element()
        }
        NoticeMessage::Failed(text) => div().child(text.clone()).into_any_element(),
    };
    let view = cx.entity().downgrade();
    let dismiss = view.clone();
    NotificationBar::new(message)
        .failed(failed)
        .when_some(notice.reveal.clone(), |this, path| {
            this.action(
                Button::new("notice-show-in-folder", "Show in folder")
                    .with_variant(ButtonVariant::Outline)
                    .with_surface(color::FLOAT)
                    .on_click(move |_, _, cx: &mut App| {
                        cx.reveal_path(&path);
                    }),
            )
        })
        .on_dismiss(move |_, _, cx| {
            let _ = dismiss.update(cx, |this, cx| this.dismiss_notice(cx));
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_sizes_read_as_on_paper() {
        assert_eq!(count(0), "0");
        assert_eq!(count(214), "214");
        assert_eq!(count(1_906), "1,906");
        assert_eq!(count(1_234_567), "1,234,567");
        assert_eq!(file_size(4_404_019), "4.2 MB");
        assert_eq!(file_size(120 * 1024), "0.1 MB");
        assert_eq!(file_size(5 * 1024 * 1024 * 1024), "5.0 GB");
        assert_eq!(plural(1, "review"), "review");
        assert_eq!(plural(12, "suggestion"), "suggestions");
    }
}
