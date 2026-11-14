//! The export dialog (ROADMAP M3.16; `docs/DESIGN.md` → "Settings" →
//! **Export dialog**; Paper `HAG-0`), opened by Export… in Settings' panel.
//!
//! "Export transactions" with CSV / JSON on the right; Dates (two mono
//! fields), Accounts, Categories and Tags (selects); then "Excluded
//! transactions · with their reason" (off) and "Notes and tags · including
//! private ones" (on); a caption with the counts (transactions and rows:
//! each split line is a row carrying its parent's ID) and where the columns
//! are described (`docs/EXPORT.md`); "Saved where you choose. Nothing is
//! uploaded." with Cancel / "Export…", which opens the system's save
//! dialog and writes there (`export::to_file`: `0600`, never over the
//! database). The same export as `coffer export`; the notes toggle sets
//! both its `--include-notes` and `--include-private-tags`.
//!
//! Composed for M3.16 (not drawn): the dates start at the ledger's first
//! and last days, so the default exports everything; an empty date field
//! means no bound; a date that doesn't read, or a filter core refuses, is
//! said in `danger` and Export… is disabled; private tags are offered only
//! while notes and tags are included. Opening it from Transactions with the
//! table's filters waits for M3.3.

use std::path::PathBuf;

use chrono::NaiveDate;
use coffer_core::category::CategoryKey;
use coffer_core::export::{Counts, Filter, Format, Options};
use coffer_core::id::AccountId;
use coffer_core::tag::TagName;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, IntoElement, ParentElement as _,
    Render, SharedString, Styled as _, Subscription, Task, Window, div,
};

use crate::accounts::DialogClosed;
use crate::format::{field_date, parse_date};
use crate::theme::tokens::{color, font, size, spacing, typography};
use crate::ui::button::{Button, ButtonVariant};
use crate::ui::dialog::Dialog;
use crate::ui::field::{FieldRow, SelectField, error_caption, text_input};
use crate::ui::menu::{MenuGroup, MenuOption, Select, SelectState};
use crate::ui::segmented::Segmented;
use crate::ui::text::TextRoleExt as _;
use crate::ui::toggle::Toggle;
use gpui_kit::component::Disableable as _;

use super::data::ExportChoices;
use super::table::{count, mono, plain, rich};
use super::{SettingsView, sentence};

/// The select key meaning "no filter".
const ANY: &str = "*";

/// Which of the dialog's selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Which {
    Account,
    Category,
    Tag,
}

/// The export dialog.
pub struct ExportDialog {
    settings: Entity<SettingsView>,
    choices: ExportChoices,
    format: Format,
    from: Entity<InputState>,
    to: Entity<InputState>,
    account: Option<AccountId>,
    category: Option<String>,
    tag: Option<String>,
    include_excluded: bool,
    include_notes: bool,
    selects: [SelectState; 3],
    counts: Option<Counts>,
    /// Why the export can't be made as it stands.
    problem: Option<SharedString>,
    focus: FocusHandle,
    today: NaiveDate,
    _subscriptions: Vec<Subscription>,
    _task: Option<Task<()>>,
}

impl EventEmitter<DialogClosed> for ExportDialog {}

impl ExportDialog {
    pub fn new(
        settings: Entity<SettingsView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (choices, problem, today) = {
            let view = settings.read(cx);
            let today = view.clock().today;
            match view.store() {
                Ok(store) => match store.export_choices() {
                    Ok(choices) => (choices, None, today),
                    Err(error) => (
                        ExportChoices::default(),
                        Some(sentence(&error.to_string())),
                        today,
                    ),
                },
                // Sample data: the choices and counts drawn on Paper; Export…
                // says it can't write.
                Err(_) => (super::fixture::export_choices(), None, today),
            }
        };
        let (first, last) = choices.span.unwrap_or((today, today));
        let from = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("First")
                .default_value(field_date(first))
        });
        let to = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Last")
                .default_value(field_date(last.max(today)))
        });
        let subscriptions = [&from, &to]
            .into_iter()
            .map(|input| {
                cx.subscribe_in(
                    input,
                    window,
                    |this: &mut Self, _, event: &InputEvent, _, cx| {
                        if let InputEvent::Change = event {
                            this.recount(cx);
                        }
                    },
                )
            })
            .collect();
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let mut dialog = Self {
            settings,
            choices,
            format: Format::Csv,
            from,
            to,
            account: None,
            category: None,
            tag: None,
            include_excluded: false,
            include_notes: true,
            selects: [
                SelectState::new(cx),
                SelectState::new(cx),
                SelectState::new(cx),
            ],
            counts: None,
            problem,
            focus,
            today,
            _subscriptions: subscriptions,
            _task: None,
        };
        if dialog.problem.is_none() {
            dialog.recount(cx);
        }
        dialog
    }

    pub fn format(&self) -> Format {
        self.format
    }

    pub fn counts(&self) -> Option<Counts> {
        self.counts
    }

    pub fn problem(&self) -> Option<&SharedString> {
        self.problem.as_ref()
    }

    /// The export the fields describe, or what's wrong with them.
    pub fn options(&self, cx: &Context<Self>) -> Result<Options, SharedString> {
        let date =
            |input: &Entity<InputState>, which: &str| -> Result<Option<NaiveDate>, SharedString> {
                let text = input.read(cx).value().to_string();
                if text.trim().is_empty() {
                    return Ok(None);
                }
                parse_date(&text, self.today).map(Some).ok_or_else(|| {
                    SharedString::from(format!(
                        "The {which} date isn't one Coffer can read. Type it as 1 Jan 2026."
                    ))
                })
            };
        let category = match &self.category {
            Some(key) => vec![CategoryKey::new(key).map_err(|error| sentence(&error.to_string()))?],
            None => Vec::new(),
        };
        let tag = match &self.tag {
            Some(name) => vec![TagName::new(name).map_err(|error| sentence(&error.to_string()))?],
            None => Vec::new(),
        };
        Ok(Options {
            filter: Filter {
                from: date(&self.from, "start")?,
                to: date(&self.to, "end")?,
                accounts: self.account.into_iter().collect(),
                categories: category,
                tags: tag,
            },
            include_notes: self.include_notes,
            include_private_tags: self.include_notes,
            include_excluded: self.include_excluded,
        })
    }

    /// Counts what the export would hold, or says why it can't be made.
    fn recount(&mut self, cx: &mut Context<Self>) {
        let options = match self.options(cx) {
            Ok(options) => options,
            Err(problem) => {
                self.counts = None;
                self.problem = Some(problem);
                cx.notify();
                return;
            }
        };
        let result = match self.settings.read(cx).store() {
            Ok(store) => store
                .export_count(&options)
                .map_err(|error| sentence(&error.to_string())),
            Err(_) => Ok(super::fixture::export_counts()),
        };
        match result {
            Ok(counts) => {
                self.counts = Some(counts);
                self.problem = None;
            }
            Err(problem) => {
                self.counts = None;
                self.problem = Some(problem);
            }
        }
        cx.notify();
    }

    pub fn set_format(&mut self, format: Format, cx: &mut Context<Self>) {
        self.format = format;
        cx.notify();
    }

    pub fn set_include_excluded(&mut self, on: bool, cx: &mut Context<Self>) {
        self.include_excluded = on;
        self.recount(cx);
    }

    pub fn set_include_notes(&mut self, on: bool, cx: &mut Context<Self>) {
        self.include_notes = on;
        // A private tag can't be filtered by once private tags stay out.
        if !on
            && let Some(tag) = &self.tag
            && self
                .choices
                .tags
                .iter()
                .any(|(name, private)| name == tag && *private)
        {
            self.tag = None;
        }
        self.recount(cx);
    }

    /// "Export…": the system's save dialog, then the file.
    pub fn export(&mut self, cx: &mut Context<Self>) {
        let options = match self.options(cx) {
            Ok(options) => options,
            Err(problem) => {
                self.problem = Some(problem);
                cx.notify();
                return;
            }
        };
        let store = match self.settings.read(cx).store() {
            Ok(store) => store.clone(),
            Err(error) => {
                self.problem = Some(error);
                cx.notify();
                return;
            }
        };
        let format = self.format;
        let extension = match format {
            Format::Csv => "csv",
            Format::Json => "json",
        };
        let name = format!(
            "coffer-export-{}.{extension}",
            self.today.format("%Y-%m-%d")
        );
        let dir = std::env::home_dir().unwrap_or_default();
        let target = cx.prompt_for_new_path(&dir, Some(&name));
        self._task = Some(cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = target.await else {
                return;
            };
            let path: PathBuf = path;
            let result = store.export(format, &options, &path);
            let _ = this.update(cx, |this, cx| match result {
                Ok(rows) => {
                    this.settings
                        .update(cx, |settings, cx| settings.exported(rows, path, cx));
                    cx.emit(DialogClosed);
                }
                Err(error) => {
                    this.problem = Some(sentence(&format!("Couldn't export: {error}")));
                    cx.notify();
                }
            });
        }));
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }

    fn index(which: Which) -> usize {
        match which {
            Which::Account => 0,
            Which::Category => 1,
            Which::Tag => 2,
        }
    }

    fn groups(&self, which: Which) -> Vec<MenuGroup> {
        let (any, options): (&str, Vec<MenuOption>) = match which {
            Which::Account => (
                "All accounts",
                self.choices
                    .accounts
                    .iter()
                    .map(|(id, name)| MenuOption::new(id.to_string(), name.clone()))
                    .collect(),
            ),
            Which::Category => {
                let mut group = String::new();
                let options = self
                    .choices
                    .categories
                    .iter()
                    .map(|(key, label, is_group)| {
                        if *is_group {
                            group = label.clone();
                            MenuOption::new(key.clone(), label.clone()).hint("all of it")
                        } else {
                            MenuOption::new(key.clone(), label.clone()).hint(group.clone())
                        }
                    })
                    .collect();
                ("All categories", options)
            }
            Which::Tag => (
                "Any tag",
                self.choices
                    .tags
                    .iter()
                    .filter(|(_, private)| self.include_notes || !private)
                    .map(|(name, private)| {
                        let option = MenuOption::new(name.clone(), name.clone());
                        if *private {
                            option.hint("private")
                        } else {
                            option
                        }
                    })
                    .collect(),
            ),
        };
        vec![
            MenuGroup::new(None, vec![MenuOption::new(ANY, any)]),
            MenuGroup::new(None, options),
        ]
    }

    fn chosen(&self, which: Which) -> SharedString {
        match which {
            Which::Account => self.account.map(|id| id.to_string()),
            Which::Category => self.category.clone(),
            Which::Tag => self.tag.clone(),
        }
        .unwrap_or_else(|| ANY.to_owned())
        .into()
    }

    fn choose(&mut self, which: Which, key: &SharedString, cx: &mut Context<Self>) {
        let value = (key.as_ref() != ANY).then(|| key.to_string());
        match which {
            Which::Account => {
                self.account = value.and_then(|id| {
                    self.choices
                        .accounts
                        .iter()
                        .map(|(account, _)| *account)
                        .find(|account| account.to_string() == id)
                })
            }
            Which::Category => self.category = value,
            Which::Tag => self.tag = value,
        }
        self.recount(cx);
    }

    fn select(&self, which: Which, cx: &mut Context<Self>) -> Select {
        let groups = self.groups(which);
        let moved = groups.clone();
        let chosen = self.chosen(which);
        let open_chosen = chosen.clone();
        let label = groups
            .iter()
            .flat_map(|group| group.options.iter())
            .find(|option| option.key == chosen)
            .map(|option| option.label.clone())
            .unwrap_or_default();
        let (id, name) = match which {
            Which::Account => ("export-accounts", "Accounts"),
            Which::Category => ("export-categories", "Categories"),
            Which::Tag => ("export-tags", "Tags"),
        };
        let view = cx.entity().downgrade();
        let (choose, step) = (view.clone(), view.clone());
        let index = Self::index(which);
        Select::new(
            id,
            name,
            SelectField::new(format!("{id}-field"), label),
            groups,
            Some(chosen),
            &self.selects[index],
        )
        .menu_width(size::DIALOG - spacing::S4 * 2. - size::FIELD_LABEL - spacing::S2)
        .max_height(size::CONTROL * 9.)
        .on_toggle(move |open, window, cx| {
            let _ = view.update(cx, |this, cx| {
                this.selects[index].set_open(open, Some(&open_chosen), window, cx);
                cx.notify();
            });
        })
        .on_choose(move |key, window, cx| {
            let _ = choose.update(cx, |this, cx| {
                this.selects[index].set_open(false, None, window, cx);
                this.choose(which, key, cx);
            });
        })
        .on_move(move |delta, _, cx| {
            let _ = step.update(cx, |this, cx| {
                this.selects[index].move_highlight(&moved, delta);
                cx.notify();
            });
        })
    }
}

/// A toggle row: the label (and a `text-muted` hint) taking the width, the
/// toggle at the end (Paper `HD5-0`).
fn toggle_row(label: &str, hint: &str, toggle: Toggle) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S2)
        .h(size::CONTROL)
        .child(
            rich(
                typography::BODY,
                color::TEXT,
                vec![
                    plain(label.to_owned()),
                    plain(format!(" · {hint}")).color(color::TEXT_MUTED),
                ],
            )
            .flex_1()
            .min_w_0(),
        )
        .child(toggle)
}

impl Render for ExportDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let format = Segmented::new(
            "export-format",
            match self.format {
                Format::Csv => "csv",
                Format::Json => "json",
            },
        )
        .segment("csv", "CSV")
        .segment("json", "JSON")
        .on_choose({
            let view = cx.entity().downgrade();
            move |key, _, cx| {
                let format = if key.as_ref() == "json" {
                    Format::Json
                } else {
                    Format::Csv
                };
                let _ = view.update(cx, |this, cx| this.set_format(format, cx));
            }
        });
        let date = |input: &Entity<InputState>, id: &'static str| {
            text_input(input).id(id).flex_1().font_family(font::MONO)
        };
        let fields = div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .child(FieldRow::new(
                "Dates",
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(spacing::S2)
                    .child(date(&self.from, "export-from"))
                    .child(
                        div()
                            .flex_none()
                            .text_role(typography::BODY)
                            .text_color(color::TEXT_MUTED)
                            .child("–"),
                    )
                    .child(date(&self.to, "export-to")),
            ))
            .child(FieldRow::new("Accounts", self.select(Which::Account, cx)))
            .child(FieldRow::new(
                "Categories",
                self.select(Which::Category, cx),
            ))
            .child(FieldRow::new("Tags", self.select(Which::Tag, cx)));
        let toggles = div()
            .flex()
            .flex_col()
            .pt(spacing::S2)
            .border_t(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .child(toggle_row(
                "Excluded transactions",
                "with their reason",
                Toggle::new(
                    "export-excluded",
                    self.include_excluded,
                    "Excluded transactions",
                )
                .on_change(
                    cx.listener(|this, on: &bool, _, cx| this.set_include_excluded(*on, cx)),
                ),
            ))
            .child(toggle_row(
                "Notes and tags",
                "including private ones",
                Toggle::new("export-notes", self.include_notes, "Notes and tags").on_change(
                    cx.listener(|this, on: &bool, _, cx| this.set_include_notes(*on, cx)),
                ),
            ));
        let summary = self.counts.map(|counts| {
            let strong = |n: usize| mono(count(n as u64)).color(color::TEXT_SECONDARY);
            rich(
                typography::CAPTION,
                color::TEXT_MUTED,
                vec![
                    strong(counts.transactions),
                    plain(format!(
                        " {} in ",
                        if counts.transactions == 1 {
                            "transaction"
                        } else {
                            "transactions"
                        }
                    )),
                    strong(counts.rows),
                    plain(format!(
                        " {}: each split line is a row of its own, carrying its parent's ID. \
                         The columns are described in ",
                        if counts.rows == 1 { "row" } else { "rows" }
                    )),
                    mono("docs/EXPORT.md"),
                    plain("."),
                ],
            )
        });
        let can_export = self.counts.is_some() && self.problem.is_none();
        let on_cancel = cx.entity().downgrade();
        Dialog::new("Export transactions", &self.focus)
            .title_trailing(format)
            .body(fields)
            .body(toggles)
            .when_some(summary, |this, summary| this.body(summary))
            .when_some(self.problem.clone(), |this, problem| {
                this.body(error_caption(problem))
            })
            .footer_lead(
                div()
                    .text_role(typography::CAPTION)
                    .text_color(color::TEXT_MUTED)
                    .child("Saved where you choose. Nothing is uploaded."),
            )
            .action(
                Button::new("export-cancel", "Cancel")
                    .with_variant(ButtonVariant::Secondary)
                    .with_surface(color::FLOAT)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            )
            .action(
                Button::new("export-confirm", "Export…")
                    .with_variant(ButtonVariant::Primary)
                    .with_surface(color::FLOAT)
                    .disabled(!can_export)
                    .on_click(cx.listener(|this, _, _, cx| this.export(cx))),
            )
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| this.cancel(cx));
            })
    }
}
