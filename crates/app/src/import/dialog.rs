//! The Import dialog (ROADMAP M3.4, M3.5; `docs/DESIGN.md` → "Import";
//! Paper `5ZQ-0` for Map, `6J4-0` for File, Review and the reconciliation
//! block).
//!
//! A wide dialog (760px) over the window: the title and a caption, the
//! panel segmented control (File / Map / Review; steps not reachable yet
//! at `opacity-disabled`) and the close button; the step's body; a footer
//! caption and actions above a hairline.
//!
//! - **File**: the dashed drop zone ("Drop a statement here", "Choose
//!   file…") and RECENT IMPORTS. A file dropped anywhere on the window or
//!   chosen here is read on a background thread and opened as a
//!   [`Session`].
//! - **Map**: Preset and Account selects, a caption saying how the preset
//!   was chosen, COLUMNS (Date, Format, Time, Amount, Merchant, Details,
//!   ID, Balance), and for a mapping of your own a FILE section (Encoding,
//!   Delimiter, Decimal, Header, Skip). On the right the preview: the first
//!   10 rows as mapped, a flagged row in `danger-soft` with its text in
//!   `danger`, and the notes under it. "Save as preset…" (M3.5) opens the
//!   save form in place of the columns. Footer: Back / "Review N rows".
//! - **Review**: New / Already imported / Flagged, AFTER IMPORT (from a
//!   rehearsal of the commit, rolled back), the reconciliation block and a
//!   locked-period block; Back / "Import N transactions", or "Import and
//!   flag the mismatch". The commit runs on a background thread (core takes
//!   the backup first), then the dialog closes and [`Imports`] shows the
//!   notification with Undo.
//!
//! Without a database (snapshots) it shows the sample export and can't
//! import.

use std::path::PathBuf;

use coffer_core::account::Account;
use coffer_core::id::AccountId;
use coffer_core::import::{Committed, Error as ImportError};
use coffer_core::money::{Currency, DecimalMark};
use coffer_core::statement::Statement;
use coffer_import::SourceFile;
use coffer_import::csv::{
    AmountDraft, Column, CsvFile, DateFormat, Delimiter, SplitSigns, TextEncoding, columns,
    date_formats,
};
use coffer_import::preset::{Preset, PresetParts, slug};
use gpui_kit::component::Disableable as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AppContext as _, Context, Div, Entity, EventEmitter, ExternalPaths, FocusHandle,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Rgba, SharedString,
    Styled as _, Subscription, Task, Window, div,
};

use crate::accounts::DialogClosed;
use crate::settings::table::{Run, mono as mono_run, plain as plain_run, rich};
use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, radius, size, spacing, typography, weight};
use crate::ui::button::{Button, ButtonVariant, IconButton};
use crate::ui::dialog::Dialog;
use crate::ui::field::{FieldRow, LockedValue, SelectField, caption, error_caption, text_input};
use crate::ui::icon::Glyph;
use crate::ui::menu::{MenuGroup, MenuOption, Select, SelectState};
use crate::ui::segmented::Segmented;
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

use super::Imports;
use super::session::{Reading, SampleRow, Session, can_import_into, sentence};
use super::store::{ImportStore, Review, Setup};
use super::text::{self, Seg, Tone};

/// How many rows the Map step's preview shows (Paper: "First 10 of 312").
pub const PREVIEW_ROWS: usize = 10;

/// The select key meaning "no column".
const NONE: &str = "-";

/// The Amount select's key for a money-out and money-in pair.
const SPLIT: &str = "split";

/// The regions a saved preset can be in (`presets/README.md`).
const REGIONS: [(&str, &str); 3] = [("uk", "UK"), ("us", "US"), ("eu", "Euro area")];

/// The step on show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    File,
    Map,
    Review,
}

/// Which select.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    Preset,
    Account,
    Statement,
    Date,
    Format,
    Time,
    Amount,
    MoneyOut,
    MoneyIn,
    Merchant,
    Details,
    Id,
    Balance,
    Encoding,
    Delimiter,
    Decimal,
    Header,
    Skip,
    Region,
}

impl Pick {
    const ALL: [Pick; 19] = [
        Pick::Preset,
        Pick::Account,
        Pick::Statement,
        Pick::Date,
        Pick::Format,
        Pick::Time,
        Pick::Amount,
        Pick::MoneyOut,
        Pick::MoneyIn,
        Pick::Merchant,
        Pick::Details,
        Pick::Id,
        Pick::Balance,
        Pick::Encoding,
        Pick::Delimiter,
        Pick::Decimal,
        Pick::Header,
        Pick::Skip,
        Pick::Region,
    ];

    fn index(self) -> usize {
        Pick::ALL
            .iter()
            .position(|pick| *pick == self)
            .expect("in ALL")
    }

    /// Its row's label, which is also the select's accessible name.
    fn label(self) -> &'static str {
        match self {
            Pick::Preset => "Preset",
            Pick::Account => "Account",
            Pick::Statement => "Statement",
            Pick::Date => "Date",
            Pick::Format => "Format",
            Pick::Time => "Time",
            Pick::Amount => "Amount",
            Pick::MoneyOut => "Money out",
            Pick::MoneyIn => "Money in",
            Pick::Merchant => "Merchant",
            Pick::Details => "Details",
            Pick::Id => "ID",
            Pick::Balance => "Balance",
            Pick::Encoding => "Encoding",
            Pick::Delimiter => "Delimiter",
            Pick::Decimal => "Decimal",
            Pick::Header => "Header",
            Pick::Skip => "Skip",
            Pick::Region => "Region",
        }
    }

    fn id(self) -> String {
        format!("import-{}", self.label().to_lowercase().replace(' ', "-"))
    }
}

/// What's running in the background.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Busy {
    Loading,
    Reading(String),
    Reviewing,
    Importing,
    Saving,
}

/// The statement as mapped, and the preview's rows: worked out after each
/// change, not each frame.
struct Mapped {
    currency: Option<Currency>,
    statement: Result<Statement, String>,
    sample: Vec<SampleRow>,
}

/// The Import dialog.
pub struct ImportDialog {
    imports: Entity<Imports>,
    store: Option<ImportStore>,
    setup: Option<Setup>,
    /// What's wrong at the dialog's level: the setup or the file.
    problem: Option<SharedString>,
    step: Step,
    session: Option<Session>,
    mapped: Option<Mapped>,
    review: Option<Review>,
    busy: Option<Busy>,
    selected_account: Option<AccountId>,
    selects: Vec<SelectState>,
    /// The save-as-preset form, open on the Map step (M3.5).
    saving: bool,
    preset_name: Entity<InputState>,
    region: &'static str,
    save_problem: Option<SharedString>,
    /// Said under the Account select after a save.
    saved: Option<SharedString>,
    focus: FocusHandle,
    task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DialogClosed> for ImportDialog {}

impl ImportDialog {
    /// The dialog on its File step; with `path`, that file is read at once
    /// (a file dropped on the window). `selected_account` is the accounts
    /// panel's selection, preferred where it fits.
    pub fn new(
        imports: Entity<Imports>,
        selected_account: Option<AccountId>,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = imports.read(cx).store().cloned();
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let preset_name = cx.new(|cx| InputState::new(window, cx).placeholder("Your bank"));
        let subscriptions = vec![cx.subscribe_in(
            &preset_name,
            window,
            |this: &mut Self, _, event: &InputEvent, _, cx| {
                if let InputEvent::Change = event {
                    this.save_problem = None;
                    cx.notify();
                }
            },
        )];
        let mut dialog = Self {
            imports,
            store,
            setup: None,
            problem: None,
            step: Step::File,
            session: None,
            mapped: None,
            review: None,
            busy: None,
            selected_account,
            selects: Pick::ALL.iter().map(|_| SelectState::new(cx)).collect(),
            saving: false,
            preset_name,
            region: "uk",
            save_problem: None,
            saved: None,
            focus,
            task: None,
            _subscriptions: subscriptions,
        };
        dialog.load(path, cx);
        dialog
    }

    /// The dialog with the sample setup (snapshots, tests): the bundled
    /// presets, Paper's accounts and recent imports, read from no
    /// database. It imports only if `imports` has a store.
    pub fn sample(imports: Entity<Imports>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut dialog = Self::new(imports, None, None, window, cx);
        dialog.task = None;
        dialog.busy = None;
        dialog.setup = Some(super::fixture::setup());
        dialog
    }

    /// Chooses the account to import into, as the Account select does.
    pub fn set_account(&mut self, account: AccountId, cx: &mut Context<Self>) {
        self.choose(Pick::Account, &SharedString::from(account.to_string()), cx);
    }

    /// Types `name` into the save form's Name field.
    pub fn set_preset_name(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.preset_name
            .update(cx, |input, cx| input.set_value(name.to_owned(), window, cx));
    }

    /// Reads the presets, accounts and last imports, then `path` if given.
    fn load(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            self.setup = Some(super::fixture::setup());
            return;
        };
        self.busy = Some(Busy::Loading);
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { store.setup() })
                .await
                .map_err(|error| SharedString::from(sentence(&error.to_string())));
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(setup) => {
                        this.setup = Some(setup);
                        if let Some(path) = path {
                            this.open_path(path, cx);
                        }
                    }
                    Err(error) => this.problem = Some(error),
                }
                cx.notify();
            });
        }));
    }

    pub fn step(&self) -> Step {
        self.step
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    pub fn review(&self) -> Option<&Review> {
        self.review.as_ref()
    }

    pub fn problem(&self) -> Option<&SharedString> {
        self.problem.as_ref()
    }

    fn accounts(&self) -> &[Account] {
        self.setup
            .as_ref()
            .map_or(&[], |setup| setup.accounts.as_slice())
    }

    // Choosing the file --------------------------------------------------------

    /// "Choose file…": the system's open dialog, then the file.
    pub fn choose_file(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.open_path(path, cx));
        }));
    }

    /// Reads the file at `path` on a background thread, then opens it.
    /// Ignored until the setup has loaded (it would cancel that read), and
    /// while another file is being read.
    pub fn open_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.setup.is_none() || self.busy.is_some() {
            return;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.busy = Some(Busy::Reading(name));
        self.problem = None;
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { ImportStore::read_file(&path) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(file) => this.open_file(file, cx),
                    Err(error) => {
                        this.problem = Some(error.to_string().into());
                        cx.notify();
                    }
                }
            });
        }));
        cx.notify();
    }

    /// Opens `file` (read already): on to Map, or the reason it can't be
    /// imported, on File.
    pub fn open_file(&mut self, file: SourceFile, cx: &mut Context<Self>) {
        let Some(setup) = &self.setup else {
            return;
        };
        match Session::open(file, &setup.presets, &setup.accounts, self.selected_account) {
            Ok(session) => {
                self.session = Some(session);
                self.review = None;
                self.saved = None;
                self.problem = None;
                self.step = Step::Map;
                self.refresh();
            }
            Err(problem) => self.problem = Some(problem.into()),
        }
        cx.notify();
    }

    /// Works out the statement and the preview again after a change.
    fn refresh(&mut self) {
        let (Some(session), Some(setup)) = (&self.session, &self.setup) else {
            self.mapped = None;
            return;
        };
        let currency = session.currency(&setup.presets, &setup.accounts);
        let statement = match currency {
            Some(currency) => session.statement(&setup.presets, currency),
            // A mapping of your own is read in the account's currency.
            None => Err("Choose the account to import into.".to_owned()),
        };
        let sample = match (&statement, currency) {
            (Ok(statement), Some(currency)) => {
                session.sample(statement, &setup.presets, currency, PREVIEW_ROWS)
            }
            _ => Vec::new(),
        };
        self.mapped = Some(Mapped {
            currency,
            statement,
            sample,
        });
        self.review = None;
    }

    // Map ----------------------------------------------------------------------

    /// What stops the Review step, if anything: a choice to make, a file
    /// that doesn't read, an account in another currency.
    fn review_blocker(&self) -> Option<String> {
        let session = self.session.as_ref()?;
        let mapped = self.mapped.as_ref()?;
        let statement = match &mapped.statement {
            Ok(statement) => statement,
            Err(problem) => return Some(problem.clone()),
        };
        let Some(account) = session.account(self.accounts()) else {
            return Some("Choose the account to import into.".to_owned());
        };
        if account.currency != statement.currency() {
            return Some(format!(
                "{} is in {}, and this file is in {}: choose an account in {}.",
                account.name,
                account.currency,
                statement.currency(),
                statement.currency()
            ));
        }
        if statement.rows().is_empty() {
            return Some("The file has no rows to import.".to_owned());
        }
        None
    }

    /// "Review N rows": core's preview and the rehearsal, on a background
    /// thread.
    pub fn start_review(&mut self, cx: &mut Context<Self>) {
        if self.review_blocker().is_some() || self.busy.is_some() {
            return;
        }
        let (Some(session), Some(mapped)) = (&self.session, &self.mapped) else {
            return;
        };
        let (Some(account), Ok(statement)) = (session.account_id(), mapped.statement.clone())
        else {
            return;
        };
        let file_name = session.file().name().to_owned();
        let Some(store) = self.store.clone() else {
            self.problem = Some("This is sample data: there's no database to import into.".into());
            cx.notify();
            return;
        };
        self.busy = Some(Busy::Reviewing);
        self.problem = None;
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { store.review(account, &file_name, statement) })
                .await
                .map_err(|error| SharedString::from(sentence(&error.to_string())));
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(review) => {
                        this.review = Some(review);
                        this.step = Step::Review;
                    }
                    Err(error) => this.problem = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Shows the Review step for `review` (snapshots).
    pub fn show_review(&mut self, review: Review, cx: &mut Context<Self>) {
        self.review = Some(review);
        self.step = Step::Review;
        cx.notify();
    }

    /// The import: core's commit (backup first), on a background thread.
    pub fn import(&mut self, cx: &mut Context<Self>) {
        let (Some(review), Some(store)) = (&self.review, self.store.clone()) else {
            return;
        };
        if review.preview.is_nothing_new() || self.busy.is_some() {
            return;
        }
        // The Review step shows every flagged row and the mismatch, and its
        // button names the mismatch: pressing it is the confirmation.
        let confirmed = review.preview.needs_confirmation();
        let preview = review.preview.clone();
        let file = preview.source_file().to_owned();
        self.busy = Some(Busy::Importing);
        self.problem = None;
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { store.commit(&preview, confirmed) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(Committed::Batch(summary)) => {
                        this.imports.update(cx, |imports, cx| {
                            imports.imported(summary.batch, summary.transactions, &file, cx)
                        });
                        cx.emit(DialogClosed);
                    }
                    Ok(Committed::NothingNew) => {
                        this.problem = Some(
                            "Nothing new to import: the account already has every row.".into(),
                        );
                    }
                    Err(error) => {
                        let stale = error
                            .downcast_ref::<ImportError>()
                            .is_some_and(|error| matches!(error, ImportError::Stale));
                        this.problem = Some(if stale {
                            "The account changed since this review (another import?). Go back \
                             and review it again."
                                .into()
                        } else {
                            sentence(&format!("Couldn't import: {error}")).into()
                        });
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DialogClosed);
    }

    fn show_step(&mut self, step: Step, cx: &mut Context<Self>) {
        match step {
            Step::Review => self.start_review(cx),
            Step::Map if self.session.is_some() => {
                self.step = Step::Map;
                self.problem = None;
            }
            Step::File => {
                self.step = Step::File;
                self.saving = false;
                self.problem = None;
            }
            Step::Map => {}
        }
        cx.notify();
    }

    // Selects --------------------------------------------------------------

    fn csv_parsed(&self) -> Option<&CsvFile> {
        self.session
            .as_ref()?
            .csv()
            .and_then(|csv| csv.parsed.as_ref().ok())
    }

    fn file_columns(&self) -> Vec<Column> {
        self.csv_parsed().map(columns).unwrap_or_default()
    }

    fn choose(&mut self, pick: Pick, key: &SharedString, cx: &mut Context<Self>) {
        let key = key.to_string();
        let Some(setup) = self.setup.clone() else {
            return;
        };
        if pick == Pick::Region {
            self.region = REGIONS
                .iter()
                .find(|(code, _)| *code == key)
                .map_or("uk", |(code, _)| *code);
            self.save_problem = None;
            cx.notify();
            return;
        }
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let column = parse_column(&key);
        match pick {
            Pick::Preset => session.set_preset(setup.presets.get(&key)),
            Pick::Account => {
                let account = key
                    .parse::<i64>()
                    .ok()
                    .and_then(|id| setup.accounts.iter().find(|a| a.id.get() == id));
                session.set_account(account, &setup.presets);
                self.saved = None;
            }
            Pick::Statement => {
                if let Ok(index) = key.parse() {
                    session.set_statement(index);
                }
            }
            Pick::Date => session.edit(|draft, parsed| match parsed {
                Some(parsed) => draft.set_date(parsed, column),
                None => draft.date = column,
            }),
            Pick::Format => {
                if let Ok(format) = DateFormat::new(&key) {
                    session.edit(|draft, _| draft.date_format = Some(format));
                }
            }
            Pick::Time => session.edit(|draft, parsed| match parsed {
                Some(parsed) => draft.set_time(parsed, column),
                None => draft.time = column,
            }),
            Pick::Amount => session.edit(|draft, _| {
                draft.amount = if key == SPLIT {
                    AmountDraft::Split {
                        debit: None,
                        credit: None,
                        signs: SplitSigns::Unsigned,
                    }
                } else {
                    AmountDraft::Signed(column)
                }
            }),
            Pick::MoneyOut | Pick::MoneyIn => session.edit(|draft, _| {
                if let AmountDraft::Split { debit, credit, .. } = &mut draft.amount {
                    if pick == Pick::MoneyOut {
                        *debit = column;
                    } else {
                        *credit = column;
                    }
                }
            }),
            Pick::Merchant => session.edit(|draft, _| draft.merchant = column),
            Pick::Details => {
                session.edit(|draft, _| draft.description = column.into_iter().collect())
            }
            Pick::Id => session.edit(|draft, _| draft.external_id = column),
            Pick::Balance => session.edit(|draft, _| draft.balance = column),
            Pick::Encoding => session.edit(|draft, _| {
                draft.layout.encoding = TextEncoding::from_label(&key).unwrap_or_default();
            }),
            Pick::Delimiter => session.edit(|draft, _| {
                draft.layout.delimiter = Delimiter::ALL
                    .into_iter()
                    .find(|delimiter| key.as_bytes() == [delimiter.byte()]);
            }),
            Pick::Decimal => session.edit(|draft, _| {
                draft.decimal = if key == "," {
                    DecimalMark::Comma
                } else {
                    DecimalMark::Point
                };
            }),
            Pick::Header => session.edit(|draft, _| draft.layout.has_header = key == "yes"),
            Pick::Skip => session.edit(|draft, _| {
                draft.layout.skip_rows = key.parse().unwrap_or(0);
            }),
            Pick::Region => {}
        }
        self.refresh();
        cx.notify();
    }

    fn select(
        &self,
        pick: Pick,
        field: SelectField,
        groups: Vec<MenuGroup>,
        chosen: Option<String>,
        cx: &mut Context<Self>,
    ) -> Select {
        let index = pick.index();
        let chosen: Option<SharedString> = chosen.map(Into::into);
        let open_chosen = chosen.clone();
        let moved = groups.clone();
        let view = cx.entity().downgrade();
        let (choose, step) = (view.clone(), view.clone());
        Select::new(
            pick.id(),
            pick.label(),
            field,
            groups,
            chosen,
            &self.selects[index],
        )
        .menu_width(size::PANEL - size::FIELD_LABEL - spacing::S2)
        .max_height(size::CONTROL * 9.)
        .on_toggle(move |open, window, cx| {
            let _ = view.update(cx, |this, cx| {
                this.selects[index].set_open(open, open_chosen.as_ref(), window, cx);
                cx.notify();
            });
        })
        .on_choose(move |key, window, cx| {
            let _ = choose.update(cx, |this, cx| {
                this.selects[index].set_open(false, None, window, cx);
                this.choose(pick, key, cx);
            });
        })
        .on_move(move |delta, _, cx| {
            let _ = step.update(cx, |this, cx| {
                this.selects[index].move_highlight(&moved, delta);
                cx.notify();
            });
        })
    }

    /// A column select: the file's columns, and "None" for an optional one.
    fn column_select(
        &self,
        pick: Pick,
        chosen: Option<&Column>,
        optional: bool,
        hint: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Select {
        let mut options: Vec<MenuOption> = self
            .file_columns()
            .iter()
            .map(|column| MenuOption::new(column_key(column), column_label(column)))
            .collect();
        if let Some(column) = chosen
            && !options
                .iter()
                .any(|option| option.key == column_key(column))
        {
            options.push(
                MenuOption::new(column_key(column), column_label(column)).hint("not in the file"),
            );
        }
        let mut groups = Vec::new();
        if optional {
            groups.push(MenuGroup::new(
                None,
                vec![MenuOption::new(NONE, "None in this file")],
            ));
        }
        groups.push(MenuGroup::new(None, options));
        let field = match chosen {
            Some(column) => {
                let field = SelectField::new(format!("{}-field", pick.id()), column_label(column));
                match hint {
                    Some(hint) => field.hint(hint.to_owned()),
                    None => field,
                }
            }
            None if optional => {
                SelectField::new(format!("{}-field", pick.id()), "None in this file")
                    .placeholder(true)
            }
            None => SelectField::new(format!("{}-field", pick.id()), "Choose a column")
                .placeholder(true),
        };
        let key = chosen.map_or_else(|| NONE.to_owned(), column_key);
        self.select(pick, field, groups, Some(key), cx)
    }

    // Saving a preset (M3.5) -------------------------------------------------

    /// The ID the save form would give: `<region>-<name as a slug>`.
    fn preset_id(&self, cx: &Context<Self>) -> Option<String> {
        let name = self.preset_name.read(cx).value().to_string();
        slug(&name).map(|slug| format!("{}-{slug}", self.region))
    }

    /// The preset the save form describes, or what's wrong with it.
    fn preset(&self, cx: &Context<Self>) -> Result<Preset, String> {
        let name = self.preset_name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return Err("Name the preset: the bank and account, such as “Example Bank”.".into());
        }
        let id = self
            .preset_id(cx)
            .ok_or("Use letters or digits in the name.".to_owned())?;
        let (Some(session), Some(setup)) = (&self.session, &self.setup) else {
            return Err("Choose a file first.".into());
        };
        if setup.presets.get(&id).is_some() {
            return Err(format!(
                "There is already a preset with the ID {id}: choose another name."
            ));
        }
        let currency = session
            .account(&setup.accounts)
            .map(|account| account.currency)
            .or(self.mapped.as_ref().and_then(|mapped| mapped.currency))
            .ok_or("Choose the account first: the preset takes its currency.".to_owned())?;
        let csv = session
            .csv()
            .ok_or("Only a CSV mapping can be saved.".to_owned())?;
        let mapping = csv
            .draft
            .build(currency)
            .map_err(|error| sentence(&error.to_string()))?;
        Preset::new(PresetParts {
            id,
            name,
            region: self.region.to_owned(),
            mapping,
            ignore: Vec::new(),
            category_map: Default::default(),
            sub_account_category: None,
        })
        .map_err(|error| sentence(&error.to_string()))
    }

    pub fn open_save_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.saving = true;
        self.save_problem = None;
        if let Some(account) = self
            .session
            .as_ref()
            .and_then(|s| s.account(self.accounts()))
        {
            let name = account
                .institution
                .clone()
                .unwrap_or_else(|| account.name.clone());
            self.preset_name
                .update(cx, |input, cx| input.set_value(name, window, cx));
        }
        cx.notify();
    }

    fn close_save_form(&mut self, cx: &mut Context<Self>) {
        self.saving = false;
        self.save_problem = None;
        cx.notify();
    }

    /// "Save preset": saves it with the user's presets and sets it on the
    /// account, then reads the presets again and maps with it.
    pub fn save_preset(&mut self, cx: &mut Context<Self>) {
        let preset = match self.preset(cx) {
            Ok(preset) => preset,
            Err(problem) => {
                self.save_problem = Some(problem.into());
                cx.notify();
                return;
            }
        };
        let Some(store) = self.store.clone() else {
            self.save_problem = Some("This is sample data: there's nowhere to save it.".into());
            cx.notify();
            return;
        };
        let account = self.session.as_ref().and_then(Session::account_id);
        self.busy = Some(Busy::Saving);
        self.task = Some(cx.spawn(async move |this, cx| {
            let saved_preset = preset.clone();
            let result = cx
                .background_spawn(async move {
                    store.save_preset(&preset, account)?;
                    store.setup()
                })
                .await
                .map_err(|error| SharedString::from(sentence(&error.to_string())));
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(setup) => {
                        let account_name = account
                            .and_then(|id| setup.accounts.iter().find(|a| a.id == id))
                            .map(|a| a.name.clone());
                        if let (Some(session), Some(preset)) =
                            (this.session.as_mut(), setup.presets.get(saved_preset.id()))
                        {
                            session.set_preset(Some(preset));
                        }
                        this.saved = Some(
                            match account_name {
                                Some(account) => format!(
                                    "Saved as {}. {account} uses it from now on.",
                                    saved_preset.name()
                                ),
                                None => format!("Saved as {}.", saved_preset.name()),
                            }
                            .into(),
                        );
                        this.setup = Some(setup);
                        this.saving = false;
                        this.refresh();
                    }
                    Err(error) => this.save_problem = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// "Save a copy…": the system's save dialog, then the preset's TOML.
    pub fn save_copy(&mut self, cx: &mut Context<Self>) {
        let preset = match self.preset(cx) {
            Ok(preset) => preset,
            Err(problem) => {
                self.save_problem = Some(problem.into());
                cx.notify();
                return;
            }
        };
        let stem = preset
            .id()
            .strip_prefix(preset.region())
            .and_then(|rest| rest.strip_prefix('-'))
            .unwrap_or(preset.id())
            .to_owned();
        let dir = std::env::home_dir().unwrap_or_default();
        let target = cx.prompt_for_new_path(&dir, Some(&format!("{stem}.toml")));
        self.task = Some(cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = target.await else {
                return;
            };
            let path: PathBuf = path;
            let shown = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let result = cx
                .background_spawn(async move { ImportStore::save_copy(&preset, &path) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.save_problem = None,
                    Err(error) => {
                        this.save_problem =
                            Some(sentence(&format!("Couldn't save {shown}: {error}")).into());
                    }
                }
                cx.notify();
            });
        }));
    }

    // Rendering -------------------------------------------------------------

    fn render_header_steps(&self, cx: &mut Context<Self>) -> Segmented {
        let key = match self.step {
            Step::File => "file",
            Step::Map => "map",
            Step::Review => "review",
        };
        let view = cx.entity().downgrade();
        Segmented::new("import-steps", key)
            .panel(size::PANEL)
            .segment("file", "File")
            .segment("map", "Map")
            .segment("review", "Review")
            .disabled("map", self.session.is_none())
            .disabled(
                "review",
                self.review.is_none()
                    && (self.review_blocker().is_some() || self.session.is_none()),
            )
            .on_choose(move |key, _, cx| {
                let step = match key.as_ref() {
                    "map" => Step::Map,
                    "review" => Step::Review,
                    _ => Step::File,
                };
                let _ = view.update(cx, |this, cx| this.show_step(step, cx));
            })
    }

    fn render_file(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let reading = match &self.busy {
            Some(Busy::Reading(name)) => Some(format!("Reading {name}…")),
            Some(Busy::Loading) => Some("Reading your accounts…".to_owned()),
            _ => None,
        };
        let well = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(size::CONTROL)
            .rounded(radius::SURFACE)
            .bg(color::FIELD)
            .child(Glyph::Download.render().text_color(color::TEXT_SECONDARY));
        let drop_target = cx.entity().downgrade();
        let zone = div()
            .id("import-drop-zone")
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(spacing::S2_5)
            .h(spacing::S6 * 7.)
            .rounded(radius::SURFACE)
            .border(size::HAIRLINE)
            .border_dashed()
            .border_color(color::BORDER_STRONG)
            .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(color::HOVER))
            .on_drop(move |paths: &ExternalPaths, _, cx| {
                if let Some(path) = paths.paths().first().cloned() {
                    let _ = drop_target.update(cx, |this, cx| this.open_path(path, cx));
                }
            })
            .child(well)
            .child(
                div()
                    .text_role(TextRole {
                        weight: weight::MEDIUM,
                        ..typography::BODY
                    })
                    .text_color(color::TEXT_STRONG)
                    .child(reading.unwrap_or_else(|| "Drop a statement here".to_owned())),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::S2)
                    .child(
                        div()
                            .text_role(typography::CAPTION)
                            .text_color(color::TEXT_MUTED)
                            .child("or"),
                    )
                    .child(
                        Button::new("import-choose-file", "Choose file…")
                            .with_variant(ButtonVariant::Outline)
                            .disabled(self.busy.is_some() || self.setup.is_none())
                            .on_click(cx.listener(|this, _, _, cx| this.choose_file(cx))),
                    ),
            );
        let mut body: Vec<AnyElement> = vec![zone.into_any_element()];
        if let Some(problem) = &self.problem {
            body.push(error_caption(problem.clone()).into_any_element());
        }
        let recent = self
            .setup
            .as_ref()
            .map(|setup| (setup.recent.clone(), setup.today))
            .filter(|(recent, _)| !recent.is_empty());
        if let Some((recent, today)) = recent {
            let mut list = div().flex().flex_col().child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(size::CONTROL)
                    .px(spacing::S2)
                    .text_color(color::TEXT_MUTED)
                    .child(Tracked::new(typography::SECTION_LABEL, "Recent imports")),
            );
            for (index, import) in recent.iter().enumerate() {
                list = list.child(
                    div()
                        .id(("import-recent", index))
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(spacing::S3)
                        .h(size::CONTROL)
                        .px(spacing::S2)
                        .rounded(radius::MD)
                        .child(cell(import.file.clone(), color::TEXT).flex_1().min_w_0())
                        .child(
                            cell(import.account.clone(), color::TEXT_MUTED).w(size::COLUMN_ACCOUNT),
                        )
                        .child(
                            cell(import.read_with.clone(), color::TEXT_MUTED)
                                .w(size::COLUMN_CATEGORY),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_none()
                                .justify_end()
                                .w(size::COLUMN_AMOUNT)
                                .text_role(typography::MONO_VALUE)
                                .text_color(color::TEXT_MUTED)
                                .child(crate::format::list_date(import.date, today)),
                        ),
                );
            }
            body.push(list.into_any_element());
        }
        body
    }

    fn render_map_left(&self, cx: &mut Context<Self>) -> Div {
        let (Some(session), Some(setup)) = (&self.session, &self.setup) else {
            return div();
        };
        let mut column = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(spacing::S2)
            .w(size::PANEL);

        // Accounts that can take the file; another currency is offered but
        // can't be chosen.
        let fixed = match session.reading() {
            Reading::Ofx(_) => self.mapped.as_ref().and_then(|mapped| mapped.currency),
            Reading::Csv(_) if session.is_preset_unchanged(&setup.presets) => {
                self.mapped.as_ref().and_then(|mapped| mapped.currency)
            }
            Reading::Csv(_) => None,
        };
        let accounts: Vec<MenuOption> = setup
            .accounts
            .iter()
            .filter(|account| can_import_into(account))
            .map(|account| {
                let other = fixed.is_some_and(|currency| currency != account.currency);
                MenuOption::new(account.id.to_string(), account.name.clone())
                    .hint(account.currency.code().to_owned())
                    .disabled(other)
            })
            .collect();
        let account_field = match session.account(&setup.accounts) {
            Some(account) => SelectField::new("import-account-field", account.name.clone()),
            None => SelectField::new("import-account-field", "Choose an account").placeholder(true),
        };
        let account_select = self.select(
            Pick::Account,
            account_field,
            vec![MenuGroup::new(None, accounts)],
            session.account_id().map(|id| id.to_string()),
            cx,
        );

        match session.reading() {
            Reading::Ofx(ofx) => {
                column = column.child(FieldRow::new("Format", LockedValue::new("OFX")));
                if ofx.statements.len() > 1 {
                    let options = ofx
                        .statements
                        .iter()
                        .enumerate()
                        .map(|(index, statement)| {
                            let label = statement_label(statement);
                            MenuOption::new(index.to_string(), label).disabled(
                                statement.kind == coffer_import::ofx::OfxStatementKind::Investment,
                            )
                        })
                        .collect();
                    let chosen = &ofx.statements[ofx.chosen];
                    column = column.child(FieldRow::new(
                        "Statement",
                        self.select(
                            Pick::Statement,
                            SelectField::new("import-statement-field", statement_label(chosen)),
                            vec![MenuGroup::new(None, options)],
                            Some(ofx.chosen.to_string()),
                            cx,
                        ),
                    ));
                }
                column = column.child(FieldRow::new("Account", account_select));
                column = column.child(field_caption(self.saved.clone().unwrap_or_else(|| {
                    "An OFX file names its own fields: there's nothing to map.".into()
                })));
            }
            Reading::Csv(csv) => {
                let unchanged = session.is_preset_unchanged(&setup.presets);
                let preset_name = csv
                    .preset
                    .as_deref()
                    .and_then(|id| setup.presets.get(id))
                    .map(|preset| preset.name().to_owned());
                let (bundled, saved): (Vec<&Preset>, Vec<&Preset>) =
                    setup.presets.iter().partition(|preset| !preset.is_saved());
                let option = |preset: &&Preset| {
                    MenuOption::new(preset.id().to_owned(), preset.name().to_owned())
                };
                let mut groups = vec![
                    MenuGroup::new(
                        None,
                        vec![MenuOption::new(NONE, "No preset").hint("your own columns")],
                    ),
                    MenuGroup::new(Some("Bundled"), bundled.iter().map(option).collect()),
                ];
                if !saved.is_empty() {
                    groups.push(MenuGroup::new(
                        Some("Saved"),
                        saved.iter().map(option).collect(),
                    ));
                }
                let preset_field = match &preset_name {
                    Some(name) if unchanged => {
                        SelectField::new("import-preset-field", name.clone())
                    }
                    Some(name) => {
                        SelectField::new("import-preset-field", name.clone()).hint("changed")
                    }
                    None => SelectField::new("import-preset-field", "No preset")
                        .hint("your own columns"),
                };
                column = column
                    .child(FieldRow::new(
                        "Preset",
                        self.select(
                            Pick::Preset,
                            preset_field,
                            groups,
                            Some(csv.preset.clone().unwrap_or_else(|| NONE.to_owned())),
                            cx,
                        ),
                    ))
                    .child(FieldRow::new("Account", account_select))
                    .child(field_caption(self.saved.clone().unwrap_or_else(|| {
                        text::mapping_caption(&csv.chosen, preset_name.as_deref(), unchanged).into()
                    })));
                column = self.render_columns(column, cx);
            }
        }
        column
    }

    /// Whether the Map step shows FILE (how the file is split): for a
    /// mapping of your own, or a file the preset can't split.
    fn shows_layout(&self) -> bool {
        let (Some(session), Some(setup)) = (&self.session, &self.setup) else {
            return false;
        };
        session.csv().is_some_and(|csv| {
            csv.preset.is_none()
                || !session.is_preset_unchanged(&setup.presets)
                || csv.parsed.is_err()
        })
    }

    /// COLUMNS: Date, Format, Time, Amount (or Money out and Money in),
    /// Merchant, Details, ID, Balance.
    fn render_columns(&self, column: Div, cx: &mut Context<Self>) -> Div {
        let Some(csv) = self.session.as_ref().and_then(Session::csv) else {
            return column;
        };
        let draft = csv.draft.clone();
        let mut column = column.child(section_label("Columns"));

        column = column.child(FieldRow::new(
            "Date",
            self.column_select(Pick::Date, draft.date.as_ref(), false, None, cx),
        ));
        // Format: the formats that read the column's values, as people
        // write them; every common one when none does.
        let mut formats = match (draft.date.as_ref(), self.csv_parsed()) {
            (Some(date), Some(parsed)) => date_formats(parsed, date),
            _ => Vec::new(),
        };
        let readable = !formats.is_empty();
        if !readable {
            formats = DateFormat::common();
        }
        if let Some(current) = &draft.date_format
            && !formats.contains(current)
        {
            formats.push(current.clone());
        }
        let format_options = formats
            .iter()
            .map(|format| {
                MenuOption::new(
                    format.pattern().to_owned(),
                    text::pattern_label(format.pattern()),
                )
            })
            .collect();
        let format_field = match &draft.date_format {
            Some(format) => {
                SelectField::new("import-format-field", text::pattern_label(format.pattern()))
                    .mono(true)
            }
            None => SelectField::new("import-format-field", "Choose the format").placeholder(true),
        };
        column = column.child(FieldRow::new(
            "Format",
            self.select(
                Pick::Format,
                format_field,
                vec![MenuGroup::new(
                    (!readable).then_some("No format reads every date"),
                    format_options,
                )],
                draft
                    .date_format
                    .as_ref()
                    .map(|format| format.pattern().to_owned()),
                cx,
            ),
        ));
        column = column.child(FieldRow::new(
            "Time",
            self.column_select(Pick::Time, draft.time.as_ref(), true, None, cx),
        ));

        match &draft.amount {
            AmountDraft::Signed(chosen) => {
                let mut options: Vec<MenuOption> = self
                    .file_columns()
                    .iter()
                    .map(|column| {
                        MenuOption::new(column_key(column), column_label(column)).hint("signed")
                    })
                    .collect();
                if let Some(column) = chosen
                    && !options
                        .iter()
                        .any(|option| option.key == column_key(column))
                {
                    options.push(
                        MenuOption::new(column_key(column), column_label(column))
                            .hint("not in the file"),
                    );
                }
                let groups = vec![
                    MenuGroup::new(None, options),
                    MenuGroup::new(
                        Some("Two columns"),
                        vec![MenuOption::new(SPLIT, "Money out and money in")],
                    ),
                ];
                let field = match chosen {
                    Some(column) => {
                        SelectField::new("import-amount-field", column_label(column)).hint("signed")
                    }
                    None => {
                        SelectField::new("import-amount-field", "Choose a column").placeholder(true)
                    }
                };
                column = column.child(FieldRow::new(
                    "Amount",
                    self.select(
                        Pick::Amount,
                        field,
                        groups,
                        chosen.as_ref().map(column_key),
                        cx,
                    ),
                ));
            }
            AmountDraft::Split {
                debit,
                credit,
                signs,
            } => {
                let groups = vec![
                    MenuGroup::new(None, vec![MenuOption::new(SPLIT, "Money out and money in")]),
                    MenuGroup::new(
                        Some("One signed column"),
                        self.file_columns()
                            .iter()
                            .map(|column| {
                                MenuOption::new(column_key(column), column_label(column))
                                    .hint("signed")
                            })
                            .collect(),
                    ),
                ];
                column = column.child(FieldRow::new(
                    "Amount",
                    self.select(
                        Pick::Amount,
                        SelectField::new("import-amount-field", "Two columns").hint(match signs {
                            SplitSigns::Unsigned => "positive amounts",
                            SplitSigns::Signed => "signed",
                        }),
                        groups,
                        Some(SPLIT.to_owned()),
                        cx,
                    ),
                ));
                column = column
                    .child(FieldRow::new(
                        "Money out",
                        self.column_select(Pick::MoneyOut, debit.as_ref(), false, None, cx),
                    ))
                    .child(FieldRow::new(
                        "Money in",
                        self.column_select(Pick::MoneyIn, credit.as_ref(), false, None, cx),
                    ));
            }
        }
        column
            .child(FieldRow::new(
                "Merchant",
                self.column_select(Pick::Merchant, draft.merchant.as_ref(), true, None, cx),
            ))
            .child(FieldRow::new(
                "Details",
                self.column_select(Pick::Details, draft.description.first(), false, None, cx),
            ))
            .child(FieldRow::new(
                "ID",
                self.column_select(Pick::Id, draft.external_id.as_ref(), true, None, cx),
            ))
            .child(FieldRow::new(
                "Balance",
                self.column_select(Pick::Balance, draft.balance.as_ref(), true, None, cx),
            ))
    }

    /// FILE: how the file is split, for a mapping of your own (a preset
    /// says it already).
    fn render_layout(&self, column: Div, cx: &mut Context<Self>) -> Div {
        let Some(csv) = self.session.as_ref().and_then(Session::csv) else {
            return column;
        };
        let draft = &csv.draft;
        let parsed = csv.parsed.as_ref().ok();
        let used_encoding = parsed.map(CsvFile::encoding);
        let encoding_label = |encoding: TextEncoding| match encoding {
            TextEncoding::Auto => "Automatic".to_owned(),
            other => other.name().to_owned(),
        };
        let encodings = [
            ("auto", TextEncoding::Auto),
            ("utf-8", TextEncoding::Utf8),
            ("utf-16le", TextEncoding::Utf16Le),
            ("utf-16be", TextEncoding::Utf16Be),
            ("windows-1252", TextEncoding::Windows1252),
            ("iso-8859-15", TextEncoding::Iso8859_15),
        ];
        let encoding_key = encodings
            .iter()
            .find(|(_, encoding)| *encoding == draft.layout.encoding)
            .map_or("auto", |(key, _)| *key);
        let mut encoding_field = SelectField::new(
            "import-encoding-field",
            encoding_label(draft.layout.encoding),
        );
        if draft.layout.encoding == TextEncoding::Auto
            && let Some(used) = used_encoding
        {
            encoding_field = encoding_field.hint(used.name().to_owned());
        }
        let encoding = self.select(
            Pick::Encoding,
            encoding_field,
            vec![MenuGroup::new(
                None,
                encodings
                    .iter()
                    .map(|(key, encoding)| MenuOption::new(*key, encoding_label(*encoding)))
                    .collect(),
            )],
            Some(encoding_key.to_owned()),
            cx,
        );

        let delimiter_label = |delimiter: Option<Delimiter>| match delimiter {
            None => "Detect it".to_owned(),
            Some(Delimiter::Comma) => "Comma".to_owned(),
            Some(Delimiter::Semicolon) => "Semicolon".to_owned(),
            Some(Delimiter::Tab) => "Tab".to_owned(),
            Some(Delimiter::Pipe) => "Pipe".to_owned(),
        };
        let delimiter_key = |delimiter: Option<Delimiter>| {
            delimiter.map_or_else(|| "auto".to_owned(), |d| char::from(d.byte()).to_string())
        };
        let mut delimiter_field = SelectField::new(
            "import-delimiter-field",
            delimiter_label(draft.layout.delimiter),
        );
        if draft.layout.delimiter.is_none()
            && let Some(parsed) = parsed
        {
            delimiter_field =
                delimiter_field.hint(delimiter_label(Some(parsed.delimiter())).to_lowercase());
        }
        let delimiter = self.select(
            Pick::Delimiter,
            delimiter_field,
            vec![MenuGroup::new(
                None,
                std::iter::once(None)
                    .chain(Delimiter::ALL.into_iter().map(Some))
                    .map(|d| MenuOption::new(delimiter_key(d), delimiter_label(d)))
                    .collect(),
            )],
            Some(delimiter_key(draft.layout.delimiter)),
            cx,
        );

        let decimal_label = |decimal: DecimalMark| match decimal {
            DecimalMark::Point => "Point",
            DecimalMark::Comma => "Comma",
        };
        let decimal = self.select(
            Pick::Decimal,
            SelectField::new("import-decimal-field", decimal_label(draft.decimal)).hint(
                match draft.decimal {
                    DecimalMark::Point => "1,234.56",
                    DecimalMark::Comma => "1.234,56",
                },
            ),
            vec![MenuGroup::new(
                None,
                vec![
                    MenuOption::new(".", "Point").hint("1,234.56"),
                    MenuOption::new(",", "Comma").hint("1.234,56"),
                ],
            )],
            Some(
                if draft.decimal == DecimalMark::Comma {
                    ","
                } else {
                    "."
                }
                .to_owned(),
            ),
            cx,
        );

        let header_label = |yes: bool| {
            if yes {
                "First row names the columns"
            } else {
                "No header row"
            }
        };
        let header = self.select(
            Pick::Header,
            SelectField::new("import-header-field", header_label(draft.layout.has_header)),
            vec![MenuGroup::new(
                None,
                vec![
                    MenuOption::new("yes", header_label(true)),
                    MenuOption::new("no", header_label(false)),
                ],
            )],
            Some(if draft.layout.has_header { "yes" } else { "no" }.to_owned()),
            cx,
        );

        let skip_label = |n: usize| match n {
            0 => "No lines".to_owned(),
            1 => "1 line".to_owned(),
            n => format!("{n} lines"),
        };
        let skip = self.select(
            Pick::Skip,
            SelectField::new("import-skip-field", skip_label(draft.layout.skip_rows))
                .hint("before the rows"),
            vec![MenuGroup::new(
                None,
                (0..=10)
                    .map(|n| MenuOption::new(n.to_string(), skip_label(n)))
                    .collect(),
            )],
            Some(draft.layout.skip_rows.to_string()),
            cx,
        );

        column
            .child(section_label("File"))
            .child(FieldRow::new("Encoding", encoding))
            .child(FieldRow::new("Delimiter", delimiter))
            .child(FieldRow::new("Decimal", decimal))
            .child(FieldRow::new("Header", header))
            .child(FieldRow::new("Skip", skip))
    }

    fn render_preview(&self) -> Div {
        let mut column = div().flex().flex_col().flex_1().min_w_0();
        let (Some(mapped), Some(setup)) = (&self.mapped, &self.setup) else {
            return column;
        };
        let rows = mapped
            .statement
            .as_ref()
            .map_or(0, |statement| statement.rows().len());
        column = column.child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(spacing::S2)
                .h(size::CONTROL)
                .child(
                    div()
                        .text_role(typography::SECTION_TITLE)
                        .text_color(color::TEXT)
                        .child("Preview"),
                )
                .when(rows > 0, |this| {
                    this.child(rich_segs(
                        typography::CAPTION,
                        color::TEXT_MUTED,
                        &[
                            Seg {
                                text: "First ".into(),
                                mono: false,
                            },
                            Seg {
                                text: text::count(rows.min(PREVIEW_ROWS)),
                                mono: true,
                            },
                            Seg {
                                text: " of ".into(),
                                mono: false,
                            },
                            Seg {
                                text: text::count(rows),
                                mono: true,
                            },
                            Seg {
                                text: if rows == 1 { " row" } else { " rows" }.into(),
                                mono: false,
                            },
                        ],
                    ))
                }),
        );
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S3)
            .h(size::CONTROL)
            .px(spacing::S2)
            .border_b(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .text_color(color::TEXT_MUTED)
            .child(
                div()
                    .flex_none()
                    .w(spacing::S6 * 2.)
                    .child(Tracked::new(typography::SECTION_LABEL, "Date")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Tracked::new(typography::SECTION_LABEL, "Merchant")),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .justify_end()
                    .w(size::COLUMN_AMOUNT)
                    .child(Tracked::new(typography::SECTION_LABEL, "Amount")),
            );
        column = column.child(header);

        let statement = match &mapped.statement {
            Ok(statement) => statement,
            Err(problem) => {
                return column.child(
                    div()
                        .pt(spacing::S3)
                        .px(spacing::S2)
                        .child(error_caption(problem.clone())),
                );
            }
        };
        let decimal = self
            .session
            .as_ref()
            .and_then(Session::csv)
            .map_or(DecimalMark::Point, |csv| csv.draft.decimal);
        let today = setup.today;
        let mut table = div().flex().flex_col().pt(spacing::S1);
        for row in mapped
            .sample
            .iter()
            .filter(|row| row.number <= PREVIEW_ROWS)
        {
            table = table.child(preview_row(row, today));
        }
        column = column.child(table);

        // The notes: the first flagged row, and the pots.
        let flagged: Vec<&SampleRow> = mapped
            .sample
            .iter()
            .filter(|row| row.read.is_err())
            .collect();
        let mut pots: Vec<String> = Vec::new();
        for row in statement.rows() {
            if let Ok(line) = &row.outcome
                && let Some(pot) = &line.sub_account
            {
                let name = pot.name.clone().unwrap_or_else(|| pot.key.clone());
                if !pots.contains(&name) {
                    pots.push(name);
                }
            }
        }
        pots.sort();
        let pots = text::pots_note(&pots);
        if flagged.is_empty() && pots.is_none() {
            return column;
        }
        let mut notes = div()
            .flex()
            .flex_col()
            .gap(spacing::S1_5)
            .mt(spacing::S1)
            .pt(spacing::S3)
            .px(spacing::S2)
            .border_t(size::HAIRLINE)
            .border_color(color::HAIRLINE);
        if let Some(first) = flagged.first()
            && let Err(row) = &first.read
        {
            let total = statement
                .rows()
                .iter()
                .filter(|row| row.outcome.is_err())
                .count();
            notes = notes.child(
                div()
                    .flex()
                    .items_start()
                    .gap(spacing::S2)
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .h(crate::theme::tokens::leading::CAPTION)
                            .child(dot(color::DANGER)),
                    )
                    .child(
                        rich_segs(
                            typography::CAPTION,
                            color::TEXT_SECONDARY,
                            &text::flag_note(
                                first.number,
                                &row.problem,
                                row.text.as_deref(),
                                decimal,
                                total - 1,
                            ),
                        )
                        .flex_1()
                        .min_w_0(),
                    ),
            );
        }
        if let Some(pots) = pots {
            notes = notes.child(div().pl(size::DOT + spacing::S2).child(caption(pots)));
        }
        column.child(notes)
    }

    fn render_save_form(&self, cx: &mut Context<Self>) -> Div {
        let id = self.preset_id(cx).unwrap_or_default();
        let region_label = REGIONS
            .iter()
            .find(|(code, _)| *code == self.region)
            .map_or("UK", |(_, label)| *label);
        let region = self.select(
            Pick::Region,
            SelectField::new("import-region-field", region_label),
            vec![MenuGroup::new(
                None,
                REGIONS
                    .iter()
                    .map(|(code, label)| MenuOption::new(*code, *label))
                    .collect(),
            )],
            Some(self.region.to_owned()),
            cx,
        );
        let account = self
            .session
            .as_ref()
            .and_then(|session| session.account(self.accounts()))
            .map(|account| account.name.clone());
        let mut form = div()
            .flex()
            .flex_col()
            .gap(spacing::S2)
            .w(size::DIALOG)
            .child(section_label("Save the mapping as a preset"))
            .child(FieldRow::new(
                "Name",
                text_input(&self.preset_name)
                    .id("import-preset-name")
                    .flex_1(),
            ))
            .child(FieldRow::new("Region", region))
            .child(FieldRow::new(
                "ID",
                LockedValue::new(if id.is_empty() { "—".to_owned() } else { id }).mono(true),
            ))
            .child(field_caption(
                match account {
                    Some(account) => format!(
                        "Saved in Coffer’s data folder, it recognises this bank’s files from now \
                         on, and {account} uses it for its next import. It holds the column \
                         layout only, never your transactions."
                    ),
                    None => "Saved in Coffer’s data folder, it recognises this bank’s files from \
                             now on. It holds the column layout only, never your transactions."
                        .to_owned(),
                }
                .into(),
            ));
        if let Some(problem) = &self.save_problem {
            form = form.child(
                div()
                    .pl(size::FIELD_LABEL + spacing::S2)
                    .child(error_caption(problem.clone())),
            );
        }
        form
    }

    fn render_review(&self) -> Vec<AnyElement> {
        let Some(review) = &self.review else {
            return Vec::new();
        };
        let preview = &review.preview;
        let counts = preview.counts();
        let figure = |label: AnyElement, value: usize, muted: bool| {
            div()
                .flex()
                .flex_col()
                .flex_none()
                .gap(spacing::S1)
                .child(label)
                .child(
                    div()
                        .text_role(typography::FIGURE)
                        .text_color(if muted {
                            color::TEXT_MUTED
                        } else {
                            color::TEXT
                        })
                        .child(text::count(value)),
                )
        };
        let label = |text: &'static str| {
            div()
                .text_role(TextRole {
                    weight: weight::MEDIUM,
                    ..typography::CAPTION
                })
                .text_color(color::TEXT_MUTED)
                .child(text)
        };
        let flagged_label = div()
            .flex()
            .items_center()
            .gap(spacing::S1_5)
            .when(counts.flagged > 0, |this| this.child(dot(color::DANGER)))
            .child(label("Flagged"));
        let gap = spacing::S6 * 2.;
        let figures = div()
            .flex()
            .items_start()
            .gap(gap)
            .px(spacing::S2)
            .child(figure(label("New").into_any_element(), counts.new, false))
            .child(
                figure(
                    label("Already imported").into_any_element(),
                    counts.already_imported,
                    true,
                )
                .ml(gap),
            )
            .child(figure(flagged_label.into_any_element(), counts.flagged, false).ml(gap))
            .when_some(
                text::already_caption(counts.already_imported),
                |this, caption| {
                    this.child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .self_end()
                            .ml(gap)
                            .justify_end()
                            .text_right()
                            .text_role(typography::CAPTION)
                            .text_color(color::TEXT_MUTED)
                            .child(caption),
                    )
                },
            );

        let mut body: Vec<AnyElement> = vec![figures.into_any_element()];

        if let Some(after) = &review.after {
            let read_with = preview
                .statement()
                .preset_id()
                .and_then(|id| self.setup.as_ref()?.presets.get(id));
            let mut list = div().flex().flex_col().child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(size::CONTROL)
                    .px(spacing::S2)
                    .text_color(color::TEXT_MUTED)
                    .child(Tracked::new(typography::SECTION_LABEL, "After import")),
            );
            list = list.child(after_row(
                "Categorised by your rules",
                after.categorized_by_rules,
                None,
            ));
            if let Some(preset) = read_with.filter(|preset| !preset.category_map().is_empty()) {
                list = list.child(after_row(
                    &format!(
                        "Categorised from {}’s categories",
                        text::bank_name(preset.name())
                    ),
                    after.categorized_by_preset,
                    None,
                ));
            }
            list = list.child(after_row("Left uncategorised", after.uncategorized, None));
            if after.mirrored > 0 || !preview.sub_accounts().is_empty() {
                list = list.child(after_row("Pot transfers paired", after.mirrored, None));
            }
            list = list.child(after_row(
                "Possible duplicates to review",
                after.duplicates,
                (after.duplicates > 0).then_some(color::WARNING),
            ));
            body.push(list.into_any_element());
        }

        let exporter = preview
            .statement()
            .preset_id()
            .and_then(|id| self.setup.as_ref()?.presets.get(id))
            .map_or_else(
                || "This file’s rows".to_owned(),
                |preset| format!("{} exports", text::bank_name(preset.name())),
            );
        body.push(block(&text::reconciliation(preview, &exporter), None).into_any_element());
        if let Some(locked) = text::locked(preview) {
            let unlock = Button::new("import-unlock", "Unlock…")
                .with_variant(ButtonVariant::Outline)
                .disabled(true);
            body.push(block(&locked, Some(unlock)).into_any_element());
        }
        body
    }
}

/// A row of the Recent imports list: one line, ellipsised.
fn cell(text: String, color: Rgba) -> Div {
    div()
        .flex_none()
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .text_role(typography::BODY)
        .text_color(color)
        .child(text)
}

/// A caps label row heading a section of the Map column (Paper: COLUMNS).
fn section_label(text: &'static str) -> Div {
    div()
        .flex()
        .flex_none()
        .items_end()
        .h(size::CONTROL)
        .pb(spacing::S1)
        .border_t(size::HAIRLINE)
        .border_color(color::HAIRLINE)
        .text_color(color::TEXT_MUTED)
        .child(Tracked::new(typography::SECTION_LABEL, text))
}

/// A caption under the selects, in the controls' lane.
fn field_caption(text: SharedString) -> Div {
    div()
        .pl(size::FIELD_LABEL + spacing::S2)
        .child(caption(text))
}

/// The 6px status dot.
fn dot(color: Rgba) -> Div {
    div()
        .flex_none()
        .size(size::DOT)
        .rounded(radius::LABEL)
        .bg(color)
}

/// One row of the Map step's preview (Paper `6GB-0`).
fn preview_row(row: &SampleRow, today: chrono::NaiveDate) -> Div {
    let base = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S3)
        .h(size::CONTROL)
        .px(spacing::S2)
        .rounded(radius::MD);
    let date_cell = |text: String| {
        div()
            .flex_none()
            .w(spacing::S6 * 2.)
            .whitespace_nowrap()
            .overflow_hidden()
            .text_role(typography::MONO_VALUE)
            .text_color(color::TEXT_MUTED)
            .child(text)
    };
    let text_cell = |text: String, muted: bool, pot: bool| {
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(spacing::S2)
            .overflow_hidden()
            .when(pot, |this| {
                this.child(Glyph::Transfer.render().text_color(color::TEXT_MUTED))
            })
            .child(
                div()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_role(typography::BODY)
                    .text_color(if muted {
                        color::TEXT_MUTED
                    } else {
                        color::TEXT
                    })
                    .child(text),
            )
    };
    let amount_cell = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_end()
        .gap(spacing::S1_5)
        .w(size::COLUMN_AMOUNT);
    match &row.read {
        Ok(line) => {
            let (merchant, pot) = text::row_text(line);
            base.child(date_cell(text::row_date(line, today)))
                .child(text_cell(merchant, pot, pot))
                .child(
                    amount_cell.child(
                        div()
                            .text_role(typography::MONO_AMOUNT)
                            .text_color(if pot { color::TEXT_MUTED } else { color::TEXT })
                            .child(text::row_amount(line)),
                    ),
                )
        }
        Err(flagged) => base
            .bg(color::DANGER_SOFT)
            .child(date_cell(
                flagged
                    .date
                    .map(|date| crate::format::list_date(date, today))
                    .unwrap_or_default(),
            ))
            .child(text_cell(
                flagged
                    .label
                    .clone()
                    .unwrap_or_else(|| format!("Row {}", row.number)),
                false,
                false,
            ))
            .child(
                amount_cell.child(dot(color::DANGER)).child(
                    div()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_role(typography::MONO_AMOUNT)
                        .text_color(color::DANGER)
                        .child(flagged.text.clone().unwrap_or_else(|| "—".to_owned())),
                ),
            ),
    }
}

/// An AFTER IMPORT row: the label in `text-secondary`, the count in mono
/// in a 96px lane, after an optional dot.
fn after_row(label: &str, count: usize, dot_color: Option<Rgba>) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S3)
        .h(size::CONTROL)
        .px(spacing::S2)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_role(typography::BODY)
                .text_color(color::TEXT_SECONDARY)
                .child(label.to_owned()),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_end()
                .gap(spacing::S1_5)
                .w(size::COLUMN_AMOUNT)
                .when_some(dot_color, |this, color| this.child(dot(color)))
                .child(
                    div()
                        .text_role(typography::MONO_AMOUNT)
                        .text_color(color::TEXT)
                        .child(text::count(count)),
                ),
        )
}

/// A Review block: the reconciliation result or the locked period (Paper
/// `6N3-0`, `6NI-0`, `6NP-0`, `6NX-0`).
fn block(block: &text::Block, action: Option<Button>) -> Div {
    let (glyph, icon, title, body, fill) = match block.tone {
        Tone::Neutral => (
            Glyph::Info,
            color::TEXT_MUTED,
            color::TEXT_STRONG,
            color::TEXT_MUTED,
            None,
        ),
        Tone::Reconciled => (
            Glyph::Reconciled,
            color::TEXT,
            color::TEXT,
            color::TEXT_MUTED,
            None,
        ),
        Tone::Mismatch => (
            Glyph::Mismatch,
            color::DANGER,
            color::DANGER,
            color::TEXT_SECONDARY,
            Some(color::DANGER_SOFT),
        ),
        Tone::Locked => (
            Glyph::LockedPeriod,
            color::WARNING,
            color::WARNING,
            color::TEXT_SECONDARY,
            Some(color::WARNING_SOFT),
        ),
    };
    div()
        .flex()
        .items_start()
        .gap(spacing::S2_5)
        .p(spacing::S3)
        .rounded(radius::SURFACE)
        .map(|this| match fill {
            Some(fill) => this.bg(fill),
            None => this.border(size::HAIRLINE).border_color(color::HAIRLINE),
        })
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .h(crate::theme::tokens::leading::UI)
                .child(glyph.render().text_color(icon)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(spacing::S0_5)
                .child(
                    div()
                        .text_role(TextRole {
                            weight: weight::STRONG,
                            ..typography::BODY
                        })
                        .text_color(title)
                        .child(block.title.clone()),
                )
                .child(rich_segs(typography::CAPTION, body, &block.body)),
        )
        .children(action)
}

/// `segs` as one wrapping paragraph in `role`, mono where marked.
pub fn rich_segs(role: TextRole, base: Rgba, segs: &[Seg]) -> Div {
    let runs: Vec<Run> = segs
        .iter()
        .map(|seg| {
            if seg.mono {
                mono_run(seg.text.clone())
            } else {
                plain_run(seg.text.clone())
            }
        })
        .collect();
    rich(role, base, runs)
}

/// A column as a select key: `n:Name` or `i:3`.
fn column_key(column: &Column) -> String {
    match column {
        Column::Named(name) => format!("n:{name}"),
        Column::Index(index) => format!("i:{index}"),
    }
}

fn parse_column(key: &str) -> Option<Column> {
    if let Some(name) = key.strip_prefix("n:") {
        Some(Column::named(name))
    } else {
        key.strip_prefix("i:")?.parse().ok().map(Column::Index)
    }
}

/// A column as the selects show it: its header name, or `Column 3`.
fn column_label(column: &Column) -> String {
    match column {
        Column::Named(name) => name.clone(),
        Column::Index(index) => format!("Column {}", index + 1),
    }
}

/// An OFX statement in the Statement select: `Bank account ending 1234 ·
/// GBP · 52`.
fn statement_label(statement: &super::session::OfxChoice) -> String {
    let kind = match statement.kind {
        coffer_import::ofx::OfxStatementKind::Bank => "Bank account",
        coffer_import::ofx::OfxStatementKind::CreditCard => "Credit card",
        coffer_import::ofx::OfxStatementKind::Investment => "Investments",
    };
    let suffix = statement
        .suffix
        .as_deref()
        .map(|suffix| format!(" ending {suffix}"))
        .unwrap_or_default();
    let currency = statement
        .currency
        .map(|currency| format!(" · {}", currency.code()))
        .unwrap_or_default();
    format!("{kind}{suffix}{currency}")
}

impl Render for ImportDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let file_name = self
            .session
            .as_ref()
            .map(|session| session.file().name().to_owned());
        let title = match (&self.step, &file_name) {
            (Step::File, _) | (_, None) => "Import a statement".to_owned(),
            (_, Some(name)) => format!("Import {name}"),
        };
        let subtitle: Vec<Seg> = match self.step {
            Step::File => vec![Seg {
                text: "CSV, OFX or QFX exported from your bank. The file stays on this computer."
                    .into(),
                mono: false,
            }],
            Step::Map => match self.session.as_ref().map(Session::reading) {
                Some(Reading::Csv(csv)) => match &csv.parsed {
                    Ok(parsed) => text::csv_caption(
                        parsed.encoding(),
                        parsed.delimiter(),
                        parsed.records().len(),
                    ),
                    Err(_) => vec![Seg {
                        text: "CSV".into(),
                        mono: false,
                    }],
                },
                Some(Reading::Ofx(ofx)) => text::ofx_caption(ofx.statements.len()),
                None => Vec::new(),
            },
            Step::Review => match &self.review {
                Some(review) => {
                    let read_with = review
                        .preview
                        .statement()
                        .preset_id()
                        .and_then(|id| self.setup.as_ref()?.presets.get(id))
                        .map(|preset| preset.name().to_owned())
                        .unwrap_or_else(|| match self.session.as_ref().map(Session::reading) {
                            Some(Reading::Ofx(_)) => "OFX".to_owned(),
                            _ => "Your mapping".to_owned(),
                        });
                    text::review_caption(
                        &read_with,
                        &review.preview.account().name,
                        text::span(&review.preview),
                    )
                }
                None => Vec::new(),
            },
        };

        let close_target = cx.entity().downgrade();
        let trailing = div()
            .flex()
            .flex_none()
            .items_start()
            .gap(spacing::S3)
            .child(self.render_header_steps(cx))
            .child(
                IconButton::new("import-close", Glyph::Dismiss, "Close").on_click(
                    move |_, _, cx| {
                        let _ = close_target.update(cx, |this, cx| this.cancel(cx));
                    },
                ),
            );

        let on_cancel = cx.entity().downgrade();
        let mut dialog = Dialog::new(title, &self.focus)
            .wide()
            .subtitle(rich_segs(typography::CAPTION, color::TEXT_MUTED, &subtitle))
            .title_trailing(trailing)
            .on_cancel(move |_, cx| {
                let _ = on_cancel.update(cx, |this, cx| {
                    if this.saving {
                        this.close_save_form(cx);
                    } else {
                        this.cancel(cx);
                    }
                });
            });

        let busy = self.busy.is_some();
        match self.step {
            Step::File => {
                for element in self.render_file(cx) {
                    dialog = dialog.body(element);
                }
                dialog = dialog
                    .footer_lead(caption(
                        "Import the same file twice and nothing is duplicated: rows already \
                         imported are recognised.",
                    ))
                    .action(
                        Button::new("import-cancel", "Cancel")
                            .with_variant(ButtonVariant::Secondary)
                            .with_surface(color::FLOAT)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                    );
            }
            Step::Map if self.saving => {
                dialog = dialog
                    .body(self.render_save_form(cx))
                    .footer_lead(caption(
                        "Save a copy… writes the same file where you choose, to keep or share.",
                    ))
                    .action(
                        Button::new("import-save-cancel", "Cancel")
                            .with_variant(ButtonVariant::Secondary)
                            .with_surface(color::FLOAT)
                            .on_click(cx.listener(|this, _, _, cx| this.close_save_form(cx))),
                    )
                    .action(
                        Button::new("import-save-copy", "Save a copy…")
                            .with_variant(ButtonVariant::Secondary)
                            .with_surface(color::FLOAT)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.save_copy(cx))),
                    )
                    .action(
                        Button::new(
                            "import-save-preset",
                            if self.busy == Some(Busy::Saving) {
                                "Saving…"
                            } else {
                                "Save preset"
                            },
                        )
                        .with_variant(ButtonVariant::Primary)
                        .with_surface(color::FLOAT)
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| this.save_preset(cx))),
                    );
            }
            Step::Map => {
                let blocker = self.review_blocker();
                let rows = self
                    .mapped
                    .as_ref()
                    .and_then(|mapped| mapped.statement.as_ref().ok())
                    .map_or(0, |statement| statement.rows().len());
                // FILE sits under the preview, where there's room, so the
                // dialog stays within the smallest window.
                let mut right = self.render_preview();
                if self.shows_layout() {
                    right = right.child(self.render_layout(
                        div().flex().flex_col().gap(spacing::S2).mt(spacing::S3),
                        cx,
                    ));
                }
                let body = div()
                    .flex()
                    .items_start()
                    .gap(spacing::S6)
                    .child(self.render_map_left(cx))
                    .child(right);
                dialog = dialog.body(body);
                if let Some(problem) = &self.problem {
                    dialog = dialog.body(error_caption(problem.clone()));
                }
                let is_csv = self.session.as_ref().is_some_and(|s| s.csv().is_some());
                let review_label = match self.busy {
                    Some(Busy::Reviewing) => "Reviewing…".to_owned(),
                    _ if rows == 0 => "Review".to_owned(),
                    _ => format!(
                        "Review {} {}",
                        text::count(rows),
                        if rows == 1 { "row" } else { "rows" }
                    ),
                };
                dialog = dialog
                    .footer_lead(caption(
                        "Nothing is saved until you import. A backup is taken first.",
                    ))
                    .when(is_csv, |dialog| {
                        dialog.action(
                            Button::new("import-save-as-preset", "Save as preset…")
                                .with_variant(ButtonVariant::Secondary)
                                .with_surface(color::FLOAT)
                                .disabled(
                                    busy || self
                                        .session
                                        .as_ref()
                                        .and_then(Session::csv)
                                        .is_none_or(|csv| csv.parsed.is_err()),
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_save_form(window, cx)
                                })),
                        )
                    })
                    .action(
                        Button::new("import-back", "Back")
                            .with_variant(ButtonVariant::Secondary)
                            .with_surface(color::FLOAT)
                            .on_click(cx.listener(|this, _, _, cx| this.show_step(Step::File, cx))),
                    )
                    .action(
                        Button::new("import-review", review_label)
                            .with_variant(ButtonVariant::Primary)
                            .with_surface(color::FLOAT)
                            .disabled(blocker.is_some() || busy)
                            .on_click(cx.listener(|this, _, _, cx| this.start_review(cx))),
                    );
            }
            Step::Review => {
                for element in self.render_review() {
                    dialog = dialog.body(element);
                }
                if let Some(problem) = &self.problem {
                    dialog = dialog.body(error_caption(problem.clone()));
                }
                let (label, nothing) = match &self.review {
                    Some(review) => (
                        text::primary_label(&review.preview),
                        review.preview.is_nothing_new(),
                    ),
                    None => (String::new(), true),
                };
                let label = if self.busy == Some(Busy::Importing) {
                    "Importing…".to_owned()
                } else {
                    label
                };
                let lead = if nothing {
                    "Every row and balance in this file is already in the account."
                } else {
                    "A backup is taken first. You can undo the whole import later."
                };
                dialog = dialog
                    .footer_lead(caption(lead))
                    .action(
                        Button::new("import-review-back", "Back")
                            .with_variant(ButtonVariant::Secondary)
                            .with_surface(color::FLOAT)
                            .on_click(cx.listener(|this, _, _, cx| this.show_step(Step::Map, cx))),
                    )
                    .action(
                        Button::new("import-commit", label)
                            .with_variant(ButtonVariant::Primary)
                            .with_surface(color::FLOAT)
                            .disabled(nothing || busy || self.store.is_none())
                            .on_click(cx.listener(|this, _, _, cx| this.import(cx))),
                    );
            }
        }
        dialog
    }
}

#[cfg(test)]
mod tests;
