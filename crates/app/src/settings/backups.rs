//! Settings · backups (ROADMAP M3.16; `docs/DESIGN.md` → "Settings" →
//! **Backups**; Paper `F63-0`).
//!
//! Two groups: Recent (the automatic backups rotation keeps as recent, and
//! every manual one) and One a month (automatic backups kept as their
//! month's newest). Each row: when (local time), why, the transactions it
//! holds, its size. "Back up now" takes a manual backup (the notification
//! says what it holds); "Restore from a file…" opens the system's file
//! dialog, then the Restore dialog. The inspector shows the selected
//! backup: what it holds, its schema against this version's, what
//! restoring would undo, Restore…, and its file (Show in folder, Save a
//! copy…).
//!
//! Composed for M3.16 (decided with the user; not drawn): reasons name the
//! import's file only where the current database still records it, and a
//! restore backup reads "Before restoring a backup" (which one isn't
//! recorded); a backup that can't be read shows "—" and the reason in its
//! inspector; with no backups the canvas says how one is taken; "Since this
//! backup" with nothing to undo says so; while "Back up now" runs its
//! button is disabled.

use coffer_core::backup::Reason;
use coffer_core::backup::detail::Detail;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::base::actions::{SelectDown, SelectUp};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::text::TextRoleExt as _;
use gpui_kit::component::Disableable as _;

use super::SettingsView;
use super::data::BackupRow;
use super::display_path;
use super::table::{
    self, caption, cell, column_label, count, file_size, group, group_title, mono, plain, plural,
    rich, row, section, selectable_row, toolbar, value_row,
};

/// When a backup was taken, in a row: `10 Nov, 14:02`, with the year when
/// it isn't this one.
pub fn taken(row: &BackupRow, today: chrono::NaiveDate) -> String {
    use chrono::Datelike as _;
    if row.local.year() == today.year() {
        row.local.format("%-d %b, %H:%M").to_string()
    } else {
        row.local.format("%-d %b %Y, %H:%M").to_string()
    }
}

/// Why a backup was taken, as runs: "Before importing `monzo.csv`".
pub fn reason(detail: &Detail) -> Vec<table::Run> {
    match detail {
        Detail::Import { file: Some(file) } => vec![plain("Before importing "), mono(file.clone())],
        Detail::Import { file: None } => vec![plain("Before an import")],
        Detail::Manual => vec![plain("Backed up by you")],
        Detail::Migration {
            from: Some(from),
            to: Some(to),
        } if from != to => vec![
            plain("Before updating Coffer"),
            plain(format!(" · schema {from} → {to}")).color(color::TEXT_MUTED),
        ],
        Detail::Migration { .. } => vec![plain("Before updating Coffer")],
        Detail::Restore => vec![plain("Before restoring a backup")],
    }
}

impl SettingsView {
    fn backup_row(&self, row: &BackupRow, cx: &mut Context<Self>) -> AnyElement {
        let path = row.backup.path.clone();
        let selected = self.selected_backup.as_ref() == Some(&path);
        selectable_row(
            format!("backup-{}", row.backup.taken_at.timestamp_millis()),
            selected,
        )
        .on_click(cx.listener(move |this, _, window, cx| {
            this.list_focus.focus(window, cx);
            this.select_backup(path.clone(), cx)
        }))
        .child(
            cell(Some(size::COLUMN_ACCOUNT))
                .text_role(typography::MONO_VALUE)
                .text_color(color::TEXT_SECONDARY)
                .child(taken(row, self.clock.today)),
        )
        .child(cell(None).child(rich(typography::BODY, color::TEXT, reason(&row.detail))))
        .child(
            cell(Some(size::COLUMN_AMOUNT))
                .justify_end()
                .text_role(typography::MONO_AMOUNT)
                .text_color(color::TEXT)
                .child(
                    row.contents
                        .map_or_else(|| "—".to_owned(), |contents| count(contents.transactions)),
                ),
        )
        .child(
            cell(Some(size::COLUMN_AMOUNT))
                .justify_end()
                .text_role(typography::MONO_AMOUNT)
                .text_color(color::TEXT_SECONDARY)
                .child(file_size(row.backup.size)),
        )
        .test_support()
        .into_any_element()
    }

    pub(super) fn render_backups(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let actions = div()
            .flex()
            .items_center()
            .gap(spacing::S2)
            .child(
                Button::new("restore-from-file", "Restore from a file…")
                    .on_click(cx.listener(|this, _, _, cx| this.restore_from_file(cx))),
            )
            .child(
                Button::new("back-up-now", "Back up now")
                    .disabled(self.backing_up)
                    .on_click(cx.listener(|this, _, _, cx| this.back_up_now(cx))),
            );

        let rows = self.backups.rows.clone();
        let (monthly, recent): (Vec<&BackupRow>, Vec<&BackupRow>) =
            rows.iter().partition(|row| row.monthly);
        let mut groups: Vec<AnyElement> = Vec::new();
        for (index, (title, members)) in [("Recent", recent), ("One a month", monthly)]
            .into_iter()
            .filter(|(_, members)| !members.is_empty())
            .enumerate()
        {
            let header = row().child(group_title(
                title,
                Some(members.len().to_string()),
                Some(size::COLUMN_ACCOUNT),
            ));
            let header = if index == 0 {
                header
                    .child(column_label("Reason", None, false))
                    .child(column_label(
                        "Transactions",
                        Some(size::COLUMN_AMOUNT),
                        true,
                    ))
                    .child(column_label("Size", Some(size::COLUMN_AMOUNT), true))
            } else {
                header
            };
            let lines: Vec<AnyElement> = members
                .into_iter()
                .map(|row| self.backup_row(row, cx))
                .collect();
            groups.push(
                group(index > 0)
                    .child(header)
                    .children(lines)
                    .into_any_element(),
            );
        }
        let empty = rows.is_empty() && self.backups_error.is_none();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(toolbar(
                "Backups",
                "Taken before every import, restore and update · the last 10 and one a month are kept",
                actions,
            ))
            .child(
                table::body("backups-body")
                    .key_context(table::LIST_CONTEXT)
                    .track_focus(&self.list_focus.clone().tab_stop(true))
                    .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.select_next(-1, cx)))
                    .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.select_next(1, cx)))
                    .when_some(self.backups_error.clone(), |this, error| {
                        this.child(
                            div()
                                .pt(spacing::S3)
                                .px(spacing::S2)
                                .text_role(typography::CAPTION)
                                .text_color(color::DANGER)
                                .child(error),
                        )
                    })
                    .when(empty, |this| {
                        this.child(div().pt(spacing::S3).px(spacing::S2).child(caption(
                            "No backups yet. Coffer takes one before every import and update; \
                             Back up now takes one at any time.",
                        )))
                    })
                    .children(groups)
                    .child(
                        div()
                            .pt(spacing::S3)
                            .px(spacing::S2)
                            .when(!empty, |this| {
                                this.border_t(size::HAIRLINE).border_color(color::HAIRLINE)
                            })
                            .child(caption(
                                "Backups stay on this computer, next to your data and readable \
                                 only by you. They're full copies, so keep any you move \
                                 elsewhere as safe as your bank statements.",
                            )),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_backup_inspector(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let row = self
            .selected_backup
            .as_ref()
            .and_then(|path| self.backups.row(path))?
            .clone();
        let muted = |text: String| plain(text).color(color::TEXT_MUTED);
        let header = table::inspector_header(
            rich(
                typography::TITLE,
                color::TEXT,
                vec![plain("Backup from "), mono(taken(&row, self.clock.today))],
            )
            .whitespace_nowrap()
            .into_any_element(),
            row.contents.map(|contents| count(contents.transactions)),
            caption(match row.contents {
                Some(_) => "Transactions in this backup",
                None => "Coffer couldn't read this backup",
            })
            .into_any_element(),
        );

        let supported = self.backups.supported;
        let mut details = section(None).child(value_row(
            "Taken",
            rich(
                typography::BODY,
                color::TEXT,
                vec![
                    mono(row.local.format("%-d %b %Y, %H:%M").to_string()),
                    muted(if row.backup.reason == Reason::Manual {
                        " · by you".into()
                    } else {
                        " · automatic".into()
                    }),
                ],
            ),
        ));
        if let Some(contents) = row.contents {
            details = details
                .child(value_row(
                    "Holds",
                    rich(
                        typography::BODY,
                        color::TEXT,
                        vec![
                            mono(count(contents.accounts)),
                            plain(format!(" {} ", plural(contents.accounts, "account"))),
                            muted("·".into()),
                            plain(" "),
                            mono(count(contents.rules)),
                            plain(format!(" {} ", plural(contents.rules, "rule"))),
                            muted("·".into()),
                            plain(" settings"),
                        ],
                    ),
                ))
                .child(value_row(
                    "Schema",
                    rich(
                        typography::BODY,
                        color::TEXT,
                        vec![
                            mono(contents.schema_version.to_string()),
                            muted(match contents.schema_version.cmp(&supported) {
                                std::cmp::Ordering::Equal => " · same as this version".into(),
                                std::cmp::Ordering::Less => {
                                    format!(" · updated to {supported} after restoring")
                                }
                                std::cmp::Ordering::Greater => {
                                    format!(" · newer than this version's {supported}")
                                }
                            }),
                        ],
                    ),
                ));
        }
        details = details.child(value_row(
            "Size",
            div()
                .text_role(typography::MONO_AMOUNT)
                .text_color(color::TEXT)
                .child(file_size(row.backup.size)),
        ));

        let since_rows: Vec<AnyElement> = match self
            .since
            .as_ref()
            .filter(|(path, _)| *path == row.backup.path)
        {
            Some((_, since)) if since.is_empty() => vec![
                caption("Nothing has been imported, edited in bulk or suggested since.")
                    .into_any_element(),
            ],
            Some((_, since)) => {
                let line = |label: Vec<table::Run>, value: String| {
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(spacing::S2)
                        .h(size::CONTROL_SM)
                        .child(
                            rich(typography::BODY, color::TEXT, label)
                                .flex_1()
                                .min_w_0(),
                        )
                        .child(
                            div()
                                .text_role(typography::MONO_VALUE)
                                .text_color(color::TEXT_SECONDARY)
                                .child(value),
                        )
                        .into_any_element()
                };
                let mut lines: Vec<AnyElement> = since
                    .imports
                    .iter()
                    .map(|import| {
                        line(
                            vec![plain("Import of "), mono(import.file.clone())],
                            format!("{} {}", count(import.rows), plural(import.rows, "row")),
                        )
                    })
                    .collect();
                if since.edits > 0 {
                    lines.push(line(vec![plain("Your edits")], count(since.edits)));
                }
                if since.suggestions > 0 {
                    lines.push(line(
                        vec![plain("Inbox suggestions")],
                        count(since.suggestions),
                    ));
                }
                lines
            }
            None => Vec::new(),
        };
        let readable = row
            .contents
            .is_some_and(|contents| contents.schema_version <= supported);
        let restore = section(Some("Since this backup"))
            .children(since_rows)
            .child(caption(
                "Restoring takes Coffer back to this point. A backup of how things are now is \
                 taken first, so you can come back.",
            ))
            .child(
                div().flex().pt(spacing::S1).child(
                    Button::new("restore-backup", "Restore…")
                        .with_variant(ButtonVariant::Primary)
                        .with_surface(color::CHROME)
                        .disabled(!readable)
                        .on_click(cx.listener(|this, _, _, cx| this.request_restore_selected(cx))),
                ),
            );

        let shown = display_path(&row.backup.path);
        let (dir, name) = match shown.rsplit_once('/') {
            Some((dir, name)) => (format!("{dir}/"), name.to_owned()),
            None => (String::new(), shown.clone()),
        };
        let reveal = row.backup.path.clone();
        let file = section(Some("File"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .text_role(typography::MONO_VALUE)
                    .text_color(color::TEXT_SECONDARY)
                    .child(dir)
                    .child(name),
            )
            .child(
                div()
                    .flex()
                    .gap(spacing::S2)
                    .child(
                        Button::new("backup-show-in-folder", "Show in folder")
                            .on_click(cx.listener(move |this, _, _, cx| this.reveal(&reveal, cx))),
                    )
                    .child(
                        Button::new("backup-save-copy", "Save a copy…")
                            .on_click(cx.listener(|this, _, _, cx| this.save_copy(cx))),
                    ),
            );

        Some(
            table::inspector("backup-inspector")
                .child(header)
                .child(details)
                .child(restore)
                .child(file)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(runs: Vec<table::Run>) -> String {
        runs.iter().map(|run| format!("{run:?}")).collect()
    }

    #[test]
    fn reasons_name_what_they_can() {
        assert!(
            text(reason(&Detail::Import {
                file: Some("monzo-november.csv".into())
            }))
            .contains("monzo-november.csv")
        );
        assert!(
            text(reason(&Detail::Migration {
                from: Some(6),
                to: Some(7)
            }))
            .contains("schema 6 → 7")
        );
        assert!(
            !text(reason(&Detail::Migration {
                from: Some(7),
                to: Some(7)
            }))
            .contains("schema")
        );
        assert!(text(reason(&Detail::Manual)).contains("Backed up by you"));
    }
}
