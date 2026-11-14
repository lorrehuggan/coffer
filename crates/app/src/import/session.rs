//! One file being imported (M3.4): how it's read, which preset and account
//! it goes with, the column mapping, and the statement that comes out.
//!
//! Pure and in memory: reading, mapping and normalizing a statement export
//! takes milliseconds, so the Map step's preview follows every change on
//! the main thread. What reads the database (the presets on disk, the
//! accounts, the preview against the account, the commit) is
//! [`super::store::ImportStore`]'s, on a background thread.
//!
//! Choosing follows `coffer import`, with the user in the loop instead of an
//! error: OFX by its format; a CSV preset by its own header row, one match
//! only; then the account's preset (`account.preset_id`, V15); otherwise
//! the columns are suggested from the header ([`MappingDraft::suggest`]) for
//! the user to check. Nothing is guessed silently: every choice is on the
//! Map step, and the preview shows what it reads.

use chrono::NaiveDate;
use coffer_core::account::{Account, AccountKind};
use coffer_core::id::AccountId;
use coffer_core::money::Currency;
use coffer_core::statement::{RowProblem, SourceLocation, Statement, StatementLine};
use coffer_import::csv::{Column, CsvFile, CsvImporter, FileLayout, MappingDraft, MappingError};
use coffer_import::ofx::{OfxImporter, OfxStatementKind};
use coffer_import::preset::{Preset, Presets};
use coffer_import::{Detect as _, Detection, Importer as _, SourceFile};

/// What the importer for a mapping that isn't a preset is called.
const OWN_MAPPING: &str = "Your mapping";

/// How the preset was chosen, for the caption under the Account select.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chosen {
    /// The file has the preset's exact header row.
    Header,
    /// Several presets have the file's header; the first is chosen and the
    /// user should check.
    Tie,
    /// The account's preset, used for its next import (V15).
    Account,
    /// The user chose it in the Preset select.
    User,
    /// No preset: the columns are the user's (suggested from the header).
    Nothing,
}

/// One statement in an OFX file, as the Statement select offers it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfxChoice {
    pub kind: OfxStatementKind,
    pub currency: Option<Currency>,
    pub transactions: usize,
    /// The last characters of the account number, as the bank wrote them.
    pub suffix: Option<String>,
}

/// An OFX file: its statements and the one chosen.
#[derive(Clone, Debug)]
pub struct OfxReading {
    pub statements: Vec<OfxChoice>,
    pub chosen: usize,
}

/// A CSV file: how it's split, the preset it started from, and the mapping.
#[derive(Clone, Debug)]
pub struct CsvReading {
    /// The file split with [`MappingDraft::layout`], or why it can't be.
    /// The error is core's or the importer's: it never holds the file's
    /// text.
    pub parsed: Result<CsvFile, String>,
    /// The preset the mapping started from.
    pub preset: Option<String>,
    pub chosen: Chosen,
    pub draft: MappingDraft,
}

/// How the file is read.
#[derive(Clone, Debug)]
pub enum Reading {
    Ofx(OfxReading),
    /// Boxed: the split file is much bigger than an OFX reading.
    Csv(Box<CsvReading>),
}

/// A file being imported.
#[derive(Clone, Debug)]
pub struct Session {
    file: SourceFile,
    reading: Reading,
    account: Option<AccountId>,
}

/// What the Map step's preview shows of one row of the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleRow {
    /// Its place among the file's rows, from 1 ("Row 8").
    pub number: usize,
    pub location: SourceLocation,
    /// The row as read, or why it can't be.
    pub read: Result<StatementLine, FlaggedRow>,
}

/// A row that can't be read, with what the file says in it: shown in the
/// preview (Paper `5ZQ-0`'s PAYPAL row), never logged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlaggedRow {
    pub problem: RowProblem,
    /// The file's text where the problem is ("8,99").
    pub text: Option<String>,
    /// Its date, when the date column reads.
    pub date: Option<NaiveDate>,
    /// Its merchant, or else its details, as written.
    pub label: Option<String>,
}

/// Whether an account can take an import: it holds transactions, isn't a
/// pot (pot rows come with their parent's file) and is open. As `coffer
/// import`.
pub fn can_import_into(account: &Account) -> bool {
    account.kind.holds_transactions()
        && account.kind != AccountKind::Pot
        && account.closed_on.is_none()
}

impl Session {
    /// Starts importing `file`: decides how to read it and, when it's clear,
    /// the account. `accounts` are the database's; `selected` is the account
    /// selected in the accounts panel, preferred when it fits. Fails only
    /// for a file that is neither OFX nor CSV Coffer can split.
    pub fn open(
        file: SourceFile,
        presets: &Presets,
        accounts: &[Account],
        selected: Option<AccountId>,
    ) -> Result<Session, String> {
        let importable: Vec<&Account> = accounts.iter().filter(|a| can_import_into(a)).collect();
        if OfxImporter::new().detect(&file) >= Detection::Format {
            let parsed = OfxImporter::new().parse(&file).map_err(|error| {
                sentence(&format!(
                    "{} isn't a readable OFX file: {error}",
                    file.name()
                ))
            })?;
            let statements: Vec<OfxChoice> = parsed
                .statements()
                .iter()
                .map(|statement| OfxChoice {
                    kind: statement.kind(),
                    currency: statement.currency(),
                    transactions: statement.transaction_count(),
                    suffix: statement.account_suffix().map(str::to_owned),
                })
                .collect();
            // The first statement Coffer can import.
            let chosen = statements
                .iter()
                .position(|s| s.kind != OfxStatementKind::Investment)
                .unwrap_or(0);
            let currency = statements.get(chosen).and_then(|s| s.currency);
            let account = default_account(&importable, currency, selected);
            return Ok(Session {
                file,
                reading: Reading::Ofx(OfxReading { statements, chosen }),
                account,
            });
        }

        let found: Vec<&Preset> = presets
            .iter()
            .filter(|preset| preset.importer().detect(&file) >= Detection::Source)
            .collect();
        let (preset, chosen) = match found.as_slice() {
            [only] => (Some(*only), Chosen::Header),
            [] => (None, Chosen::Nothing),
            several => {
                // An account whose preset is one of them settles it.
                let by_account = several.iter().copied().find(|preset| {
                    importable
                        .iter()
                        .any(|account| account.preset.as_deref() == Some(preset.id()))
                });
                (Some(by_account.unwrap_or(several[0])), Chosen::Tie)
            }
        };
        let mut session = match preset {
            Some(preset) => Session::with_preset(file, preset, chosen),
            None => {
                let layout = FileLayout::default();
                let parsed = CsvFile::read(&file, &layout);
                let readable = parsed.as_ref().is_ok_and(|parsed| parsed.width() >= 2);
                let csv_named = matches!(file.extension().as_deref(), Some("csv" | "tsv" | "txt"));
                if !readable && !csv_named {
                    return Err(format!(
                        "Coffer imports CSV, OFX and QFX files, and {} isn't one it can read.",
                        file.name()
                    ));
                }
                let draft = match &parsed {
                    Ok(parsed) => MappingDraft::suggest(parsed, layout),
                    Err(_) => MappingDraft::new(layout),
                };
                Session {
                    file,
                    reading: Reading::Csv(Box::new(CsvReading {
                        parsed: parsed.map_err(|error| sentence(&error.to_string())),
                        preset: None,
                        chosen: Chosen::Nothing,
                        draft,
                    })),
                    account: None,
                }
            }
        };

        let preset_id = session.preset_id().map(str::to_owned);
        let by_preset: Vec<&&Account> = importable
            .iter()
            .filter(|account| preset_id.is_some() && account.preset == preset_id)
            .collect();
        session.account = match by_preset.as_slice() {
            [only] => Some(only.id),
            _ => {
                let currency = preset_id
                    .as_deref()
                    .and_then(|id| presets.get(id))
                    .map(|preset| preset.mapping().currency);
                default_account(&importable, currency, selected)
            }
        };
        // No preset recognised the file by its header: the account's own
        // preset, if Coffer has it and it reads the file (a preset without
        // a header row is only ever chosen this way).
        if session.preset_id().is_none()
            && let Some(account) = session.account(accounts)
            && let Some(preset) = account.preset.as_deref().and_then(|id| presets.get(id))
            && reads(preset, &session.file)
        {
            session.use_preset(preset, Chosen::Account);
        }
        Ok(session)
    }

    fn with_preset(file: SourceFile, preset: &Preset, chosen: Chosen) -> Session {
        let mut session = Session {
            file,
            reading: Reading::Csv(Box::new(CsvReading {
                parsed: Err(String::new()),
                preset: None,
                chosen: Chosen::Nothing,
                draft: MappingDraft::new(FileLayout::default()),
            })),
            account: None,
        };
        session.use_preset(preset, chosen);
        session
    }

    /// Starts the mapping again from `preset`.
    fn use_preset(&mut self, preset: &Preset, chosen: Chosen) {
        let file = self.file.clone();
        if let Reading::Csv(csv) = &mut self.reading {
            csv.draft = MappingDraft::from_mapping(preset.mapping());
            csv.preset = Some(preset.id().to_owned());
            csv.chosen = chosen;
            csv.parsed =
                CsvFile::read(&file, &csv.draft.layout).map_err(|e| sentence(&e.to_string()));
        }
    }

    pub fn file(&self) -> &SourceFile {
        &self.file
    }

    pub fn reading(&self) -> &Reading {
        &self.reading
    }

    /// The CSV reading, if the file is CSV.
    pub fn csv(&self) -> Option<&CsvReading> {
        match &self.reading {
            Reading::Csv(csv) => Some(csv),
            Reading::Ofx(_) => None,
        }
    }

    /// The account chosen to import into.
    pub fn account_id(&self) -> Option<AccountId> {
        self.account
    }

    /// The chosen account, among `accounts`.
    pub fn account<'a>(&self, accounts: &'a [Account]) -> Option<&'a Account> {
        let id = self.account?;
        accounts.iter().find(|account| account.id == id)
    }

    /// The preset the mapping started from.
    pub fn preset_id(&self) -> Option<&str> {
        self.csv().and_then(|csv| csv.preset.as_deref())
    }

    /// Whether the mapping is the preset's own, unchanged: the statement
    /// then carries the preset's ID and category map.
    pub fn is_preset_unchanged(&self, presets: &Presets) -> bool {
        match (self.csv(), self.preset_id().and_then(|id| presets.get(id))) {
            (Some(csv), Some(preset)) => csv.draft == MappingDraft::from_mapping(preset.mapping()),
            _ => false,
        }
    }

    /// Chooses the account. A file no preset recognised takes the
    /// account's preset, unless the user chose one.
    pub fn set_account(&mut self, account: Option<&Account>, presets: &Presets) {
        self.account = account.map(|account| account.id);
        let chosen = self.csv().map(|csv| csv.chosen.clone());
        if matches!(chosen, Some(Chosen::Nothing | Chosen::Account))
            && let Some(preset) = account
                .and_then(|account| account.preset.as_deref())
                .and_then(|id| presets.get(id))
            && self.preset_id() != Some(preset.id())
            && reads(preset, &self.file)
        {
            self.use_preset(preset, Chosen::Account);
        }
    }

    /// Chooses the preset (`None`: map the columns yourself, starting from
    /// the mapping as it is).
    pub fn set_preset(&mut self, preset: Option<&Preset>) {
        match preset {
            Some(preset) => self.use_preset(preset, Chosen::User),
            None => {
                if let Reading::Csv(csv) = &mut self.reading {
                    csv.preset = None;
                    csv.chosen = Chosen::Nothing;
                }
            }
        }
    }

    /// Changes the mapping; the file is split again when its layout
    /// changed.
    pub fn edit(&mut self, change: impl FnOnce(&mut MappingDraft, Option<&CsvFile>)) {
        let file = self.file.clone();
        if let Reading::Csv(csv) = &mut self.reading {
            let before = csv.draft.layout.clone();
            change(&mut csv.draft, csv.parsed.as_ref().ok());
            if csv.draft.layout != before {
                csv.parsed =
                    CsvFile::read(&file, &csv.draft.layout).map_err(|e| sentence(&e.to_string()));
            }
        }
    }

    /// Chooses the statement of an OFX file with several.
    pub fn set_statement(&mut self, index: usize) {
        if let Reading::Ofx(ofx) = &mut self.reading
            && index < ofx.statements.len()
        {
            ofx.chosen = index;
        }
    }

    /// The currency the statement is read in: the preset's, the OFX
    /// statement's, or for a mapping of your own the account's (`None`
    /// until an account is chosen).
    pub fn currency(&self, presets: &Presets, accounts: &[Account]) -> Option<Currency> {
        match &self.reading {
            Reading::Ofx(ofx) => ofx.statements.get(ofx.chosen).and_then(|s| s.currency),
            Reading::Csv(_) if self.is_preset_unchanged(presets) => self
                .preset_id()
                .and_then(|id| presets.get(id))
                .map(|preset| preset.mapping().currency),
            Reading::Csv(_) => self.account(accounts).map(|account| account.currency),
        }
    }

    /// The importer a CSV file is read with: the preset's while the mapping
    /// is unchanged, else one over the user's mapping (no preset ID, no
    /// category map), reading amounts in `currency`.
    pub fn csv_importer(
        &self,
        presets: &Presets,
        currency: Currency,
    ) -> Option<Result<CsvImporter, String>> {
        let csv = self.csv()?;
        if self.is_preset_unchanged(presets)
            && let Some(preset) = self.preset_id().and_then(|id| presets.get(id))
        {
            return Some(Ok(preset.importer().clone()));
        }
        Some(
            csv.draft
                .build(currency)
                .map_err(|error| sentence(&error.to_string()))
                .and_then(|mapping| {
                    CsvImporter::new(OWN_MAPPING, mapping)
                        .map_err(|error: MappingError| sentence(&error.to_string()))
                }),
        )
    }

    /// The statement the file gives as mapped, reading amounts in
    /// `currency` where the mapping doesn't fix it, or what stops it: a
    /// choice still to make, a file that doesn't split, a column the file
    /// doesn't have. Messages never hold the file's text.
    pub fn statement(&self, presets: &Presets, currency: Currency) -> Result<Statement, String> {
        match &self.reading {
            Reading::Ofx(ofx) => {
                let parsed = OfxImporter::new()
                    .parse(&self.file)
                    .map_err(|error| sentence(&error.to_string()))?;
                let importer = if ofx.statements.len() > 1 {
                    OfxImporter::for_statement(ofx.chosen)
                } else {
                    OfxImporter::new()
                };
                importer
                    .normalize(parsed)
                    .map_err(|error| sentence(&error.to_string()))
            }
            Reading::Csv(csv) => {
                let parsed = csv.parsed.clone()?;
                let importer = self
                    .csv_importer(presets, currency)
                    .expect("a CSV reading")?;
                importer
                    .normalize(parsed)
                    .map_err(|error| sentence(&error.to_string()))
            }
        }
    }

    /// The preview's rows: the first `count`, then any later flagged row
    /// (whose note the preview shows), each with the file's text where its
    /// problem is.
    pub fn sample(
        &self,
        statement: &Statement,
        presets: &Presets,
        currency: Currency,
        count: usize,
    ) -> Vec<SampleRow> {
        let importer = self.csv_importer(presets, currency).and_then(Result::ok);
        let parsed = self.csv().and_then(|csv| csv.parsed.as_ref().ok());
        statement
            .rows()
            .iter()
            .enumerate()
            .filter(|(index, row)| *index < count || row.outcome.is_err())
            .map(|(index, row)| SampleRow {
                number: index + 1,
                location: row.location,
                read: match &row.outcome {
                    Ok(line) => Ok(line.clone()),
                    Err(problem) => {
                        let text = match (&importer, parsed) {
                            (Some(importer), Some(parsed)) => {
                                importer.problem_text(parsed, row.location, problem)
                            }
                            _ => None,
                        };
                        let (date, label) = self.raw_cells(row.location);
                        Err(FlaggedRow {
                            problem: problem.clone(),
                            text,
                            date,
                            label,
                        })
                    }
                },
            })
            .collect()
    }
}

impl Session {
    /// A CSV row's date (when the mapped format reads it) and its merchant
    /// or details, from the file's cells: what the preview shows of a
    /// flagged row.
    fn raw_cells(&self, location: SourceLocation) -> (Option<NaiveDate>, Option<String>) {
        let (Some(csv), SourceLocation::Line(line)) = (self.csv(), location) else {
            return (None, None);
        };
        let Ok(parsed) = &csv.parsed else {
            return (None, None);
        };
        let Some(record) = parsed.records().iter().find(|record| record.line == line) else {
            return (None, None);
        };
        let cell = |column: &Column| -> Option<String> {
            let index = match column {
                Column::Index(index) => *index,
                Column::Named(name) => parsed
                    .header()?
                    .fields
                    .iter()
                    .position(|cell| cell == name.trim())?,
            };
            record
                .fields
                .get(index)
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty())
        };
        let draft = &csv.draft;
        let date = draft
            .date
            .as_ref()
            .and_then(cell)
            .zip(draft.date_format.as_ref())
            .and_then(|(text, format)| format.parse(&text))
            .map(|(date, _)| date);
        let label = draft
            .merchant
            .as_ref()
            .and_then(cell)
            .or_else(|| draft.description.first().and_then(cell));
        (date, label)
    }
}

/// Whether `preset` reads `file`: it splits, and every column the preset
/// names is there (rows may still be flagged).
fn reads(preset: &Preset, file: &SourceFile) -> bool {
    use coffer_import::AnyImporter as _;
    preset.importer().import(file).is_ok()
}

/// The account a file goes to when nothing else says: the panel's
/// selection if it can take `currency`, else the only account that can.
fn default_account(
    importable: &[&Account],
    currency: Option<Currency>,
    selected: Option<AccountId>,
) -> Option<AccountId> {
    let fits: Vec<&&Account> = importable
        .iter()
        .filter(|account| currency.is_none_or(|currency| account.currency == currency))
        .collect();
    if let Some(selected) = selected
        && fits.iter().any(|account| account.id == selected)
    {
        return Some(selected);
    }
    match fits.as_slice() {
        [only] => Some(only.id),
        _ => None,
    }
}

/// `text` with its first letter capitalised and a full stop: core's and the
/// importers' messages are clauses.
pub fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    let mut out: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    if !out.ends_with(['.', '?', '!']) {
        out.push('.');
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::NaiveDate;
    use coffer_import::csv::{AmountDraft, Column, DateFormat};

    use super::*;

    fn fixture(path: &str) -> SourceFile {
        SourceFile::read(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures")
                .join(path),
        )
        .unwrap()
    }

    fn account(id: i64, name: &str, currency: Currency, preset: Option<&str>) -> Account {
        Account {
            id: AccountId::new(id),
            parent: None,
            name: name.to_owned(),
            institution: None,
            kind: AccountKind::Current,
            currency,
            opening_balance: None,
            reconciled_through: None,
            closed_on: None,
            preset: preset.map(str::to_owned),
        }
    }

    fn presets() -> Presets {
        Presets::bundled().unwrap()
    }

    #[test]
    fn a_preset_is_found_by_its_header_and_its_account_by_the_preset() {
        let accounts = [
            account(1, "Joint", Currency::GBP, None),
            account(2, "Current", Currency::GBP, Some("uk-monzo")),
        ];
        let session =
            Session::open(fixture("presets/uk/monzo.csv"), &presets(), &accounts, None).unwrap();
        assert_eq!(session.preset_id(), Some("uk-monzo"));
        assert_eq!(session.csv().unwrap().chosen, Chosen::Header);
        assert_eq!(session.account_id(), Some(AccountId::new(2)));
        assert!(session.is_preset_unchanged(&presets()));

        let statement = session.statement(&presets(), Currency::GBP).unwrap();
        assert_eq!(statement.preset_id(), Some("uk-monzo"));
    }

    #[test]
    fn an_unknown_layout_is_suggested_and_waits_for_the_ambiguous_date() {
        let accounts = [account(1, "Current", Currency::GBP, None)];
        let mut session = Session::open(
            fixture("csv/signed-amount.csv"),
            &presets(),
            &accounts,
            None,
        )
        .unwrap();
        assert_eq!(session.preset_id(), None);
        assert_eq!(session.account_id(), Some(AccountId::new(1)));
        let csv = session.csv().unwrap();
        assert_eq!(csv.chosen, Chosen::Nothing);
        assert_eq!(
            csv.draft.amount,
            AmountDraft::Signed(Some(Column::named("Amount")))
        );
        assert_eq!(
            session.statement(&presets(), Currency::GBP).unwrap_err(),
            "Choose the date format."
        );

        session.edit(|draft, _| draft.date_format = Some(DateFormat::new("%d/%m/%Y").unwrap()));
        let statement = session.statement(&presets(), Currency::GBP).unwrap();
        assert_eq!(statement.preset_id(), None);
        let first = statement.rows()[0].outcome.as_ref().unwrap();
        assert_eq!(first.date, NaiveDate::from_ymd_opt(2026, 11, 1).unwrap());
    }

    /// A headerless layout, which only an account's preset can choose.
    const HEADERLESS: &str = r#"
id = "uk-plain"
name = "Plain Bank"
region = "uk"
currency = "GBP"
header = false
date = { column = 1, format = "%Y-%m-%d" }
description = { column = 2 }
amount = { column = 3 }
"#;

    fn with_headerless() -> Presets {
        let (presets, errors) = Presets::bundled()
            .unwrap()
            .with_user_presets([("uk/plain.toml", HEADERLESS)]);
        assert!(errors.is_empty(), "{errors:?}");
        presets
    }

    #[test]
    fn the_accounts_preset_reads_a_file_no_header_settles() {
        let presets = with_headerless();
        let file = SourceFile::from_bytes(
            "plain.csv",
            "2026-11-23,SHOP,-10.00\n2026-11-24,CAFE,-3.40\n",
        );
        let accounts = [account(1, "Current", Currency::GBP, Some("uk-plain"))];
        let session = Session::open(file, &presets, &accounts, None).unwrap();
        assert_eq!(session.preset_id(), Some("uk-plain"));
        assert_eq!(session.csv().unwrap().chosen, Chosen::Account);
        assert_eq!(
            session
                .statement(&presets, Currency::GBP)
                .unwrap()
                .preset_id(),
            Some("uk-plain")
        );
    }

    #[test]
    fn an_accounts_preset_that_cant_read_the_file_is_left_alone() {
        // A Monzo file with a header Monzo's preset doesn't have.
        let text = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/presets/uk/monzo.csv"),
        )
        .unwrap()
        .replacen("Transaction ID", "Txn ID", 1);
        let file = SourceFile::from_bytes("monzo.csv", text);
        let accounts = [account(1, "Current", Currency::GBP, Some("uk-monzo"))];
        let session = Session::open(file, &presets(), &accounts, None).unwrap();
        assert_eq!(session.preset_id(), None);
        assert_eq!(session.csv().unwrap().chosen, Chosen::Nothing);
        // The columns are suggested instead, the renamed one included.
        assert_eq!(
            session.csv().unwrap().draft.external_id,
            None,
            "“Txn ID” isn't a name Coffer suggests"
        );
    }

    #[test]
    fn editing_a_preset_makes_it_your_mapping() {
        let accounts = [account(1, "Current", Currency::GBP, None)];
        let mut session =
            Session::open(fixture("presets/uk/monzo.csv"), &presets(), &accounts, None).unwrap();
        session.edit(|draft, _| draft.external_id = None);
        assert!(!session.is_preset_unchanged(&presets()));
        let statement = session.statement(&presets(), Currency::GBP).unwrap();
        assert_eq!(statement.preset_id(), None);
        assert!(
            statement
                .rows()
                .iter()
                .filter_map(|row| row.outcome.as_ref().ok())
                .all(|line| line.external_id.is_none() && line.category.is_none())
        );
        // Back to the preset.
        let presets = presets();
        session.set_preset(presets.get("uk-monzo"));
        assert!(session.is_preset_unchanged(&presets));
    }

    #[test]
    fn choosing_an_account_with_a_preset_reads_with_it() {
        let presets = with_headerless();
        let accounts = [
            account(1, "Other", Currency::GBP, None),
            account(2, "Current", Currency::GBP, Some("uk-plain")),
        ];
        let file = SourceFile::from_bytes("x.csv", "2026-11-23,SHOP,-10.00\n");
        let mut session = Session::open(file, &presets, &accounts, None).unwrap();
        assert_eq!(session.preset_id(), None);
        session.set_account(Some(&accounts[1]), &presets);
        assert_eq!(session.preset_id(), Some("uk-plain"));
        // A preset the user chose stays when the account changes.
        session.set_preset(None);
        session.set_preset(presets.get("uk-plain"));
        session.set_account(Some(&accounts[0]), &presets);
        assert_eq!(session.csv().unwrap().chosen, Chosen::User);
    }

    #[test]
    fn the_panels_account_is_preferred_when_it_fits() {
        let accounts = [
            account(1, "A", Currency::GBP, None),
            account(2, "B", Currency::GBP, None),
            account(3, "Euro", Currency::EUR, None),
        ];
        let open = |selected| {
            Session::open(
                fixture("csv/signed-amount.csv"),
                &presets(),
                &accounts,
                selected,
            )
            .unwrap()
            .account_id()
        };
        assert_eq!(open(None), None);
        assert_eq!(open(Some(AccountId::new(2))), Some(AccountId::new(2)));
    }

    #[test]
    fn ofx_files_list_their_statements() {
        let accounts = [account(1, "Savings", Currency::GBP, None)];
        let session =
            Session::open(fixture("ofx/bank-sgml.ofx"), &presets(), &accounts, None).unwrap();
        let Reading::Ofx(ofx) = session.reading() else {
            panic!("read as OFX");
        };
        assert_eq!(ofx.statements.len(), 1);
        assert_eq!(session.account_id(), Some(AccountId::new(1)));
        assert!(session.statement(&presets(), Currency::GBP).is_ok());
    }

    #[test]
    fn a_file_that_isnt_a_statement_is_refused() {
        let file = SourceFile::from_bytes("notes.pdf", b"%PDF-1.7 not a statement".to_vec());
        let error = Session::open(file, &presets(), &[], None).unwrap_err();
        assert_eq!(
            error,
            "Coffer imports CSV, OFX and QFX files, and notes.pdf isn't one it can read."
        );
    }

    #[test]
    fn the_sample_shows_a_flagged_rows_text() {
        let file = SourceFile::from_bytes(
            "x.csv",
            "Date,Description,Amount\n23/11/2026,A,-1.00\n24/11/2026,B,\"8,99\"\n",
        );
        let accounts = [account(1, "Current", Currency::GBP, None)];
        let session = Session::open(file, &presets(), &accounts, None).unwrap();
        let statement = session.statement(&presets(), Currency::GBP).unwrap();
        let sample = session.sample(&statement, &presets(), Currency::GBP, 10);
        assert_eq!(sample.len(), 2);
        assert_eq!(sample[1].number, 2);
        let Err(flagged) = &sample[1].read else {
            panic!("flagged");
        };
        assert_eq!(flagged.text.as_deref(), Some("8,99"));
        assert_eq!(flagged.label.as_deref(), Some("B"));
        assert_eq!(flagged.date, NaiveDate::from_ymd_opt(2026, 11, 24));
    }

    #[test]
    fn sentences() {
        assert_eq!(sentence("the file has no rows"), "The file has no rows.");
        assert_eq!(sentence("Done."), "Done.");
    }
}
