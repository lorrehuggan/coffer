//! Planning and mirroring pot transfers against a real database, through
//! `import::preview` (which runs [`plan`]) and `import::commit` (which runs
//! [`mirror`] after storing the batch's rows and before rules and transfer
//! detection), and [`mirror`] directly for what the commit never asks of it.

use chrono::NaiveTime;
use tempfile::TempDir;

use super::*;
use crate::account::NewAccount;
use crate::import::{self, Committed, RowOutcome};
use crate::money::{Currency, Money};
use crate::statement::{SourceFormat, SourceLocation, Statement};
use crate::transfer::{TransferStatus, TransferWindow};

const RAINY: &str = "pot_0000FAKEPOT00000000001";
const HOLIDAY: &str = "pot_0000FAKEPOT00000000002";

struct Fixture {
    db: Database,
    _dir: TempDir,
}

/// What one import did, read back from the database after the commit.
struct Imported {
    plan: MirrorPlan,
    batch: Option<BatchId>,
    mirrored: MirrorSummary,
    /// Pairs detection suggested in the commit, after mirroring.
    transfer_pairs: usize,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        Fixture { db, _dir: dir }
    }

    fn account(&mut self, name: &str, kind: AccountKind) -> AccountId {
        let mut new = NewAccount::new(name, kind, Currency::GBP);
        new.institution = Some("Example Bank".into());
        account::create(&mut self.db, &new).unwrap()
    }

    fn get(&self, id: AccountId) -> Account {
        account::get(&self.db, id).unwrap()
    }

    /// What a preview of `statement` says the commit would mirror
    /// ([`plan`], run by `import::preview`).
    fn plan(&self, account: AccountId, statement: Statement) -> Result<MirrorPlan, Error> {
        let preview =
            import::preview(&self.db, account, "monzo.csv", statement).map_err(sub_accounts)?;
        Ok(preview.sub_accounts().clone())
    }

    /// Previews and commits `statement` into `account` (the commit runs
    /// [`mirror`] before rules and transfer detection), then reads back
    /// what it mirrored.
    fn import(&mut self, account: AccountId, statement: Statement) -> Result<Imported, Error> {
        let preview =
            import::preview(&self.db, account, "monzo.csv", statement).map_err(sub_accounts)?;
        let plan = preview.sub_accounts().clone();
        let pots_before = self.pot_ids(account);
        let confirm = preview.needs_confirmation();
        let summary = match import::commit(&mut self.db, &preview, confirm).map_err(sub_accounts)? {
            Committed::Batch(summary) => summary,
            Committed::NothingNew => {
                return Ok(Imported {
                    plan,
                    batch: None,
                    mirrored: MirrorSummary::default(),
                    transfer_pairs: 0,
                });
            }
        };
        let batch = summary.batch;

        let created_pots: Vec<AccountId> = self
            .pot_ids(account)
            .into_iter()
            .filter(|id| !pots_before.contains(id))
            .collect();
        let conn = self.db.conn();
        let mut rows = Vec::new();
        for line in new_lines(&preview).filter(|line| line.sub_account.is_some()) {
            let (parent_row, group): (TransactionId, TransferGroupId) = conn
                .query_row(
                    "SELECT id, transfer_group_id FROM \"transaction\" \
                     WHERE batch_id = ?1 AND external_id = ?2 AND origin = 'imported'",
                    params![batch, line.external_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            let (mirrored_row, pot): (TransactionId, AccountId) = conn
                .query_row(
                    "SELECT id, account_id FROM \"transaction\" \
                     WHERE batch_id = ?1 AND external_id = ?2 AND origin = 'mirrored' \
                     AND transfer_group_id = ?3",
                    params![batch, line.external_id, group],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            rows.push(MirroredRow {
                parent_row,
                mirrored_row,
                pot,
                group,
            });
        }
        assert_eq!(summary.mirrored, rows.len());
        assert_eq!(summary.pots_created, created_pots.len());
        Ok(Imported {
            plan,
            batch: Some(batch),
            mirrored: MirrorSummary { created_pots, rows },
            transfer_pairs: summary.transfer_pairs,
        })
    }

    fn pot_ids(&self, parent: AccountId) -> Vec<AccountId> {
        pots(&self.db, parent)
            .unwrap()
            .into_iter()
            .map(|pot| pot.account.id)
            .collect()
    }

    fn pot_keyed(&self, parent: AccountId, key: &str) -> Option<Account> {
        pots(&self.db, parent)
            .unwrap()
            .into_iter()
            .find(|pot| pot.key.as_deref() == Some(key))
            .map(|pot| pot.account)
    }

    fn count(&self, sql: &str) -> i64 {
        self.db.conn().query_row(sql, [], |row| row.get(0)).unwrap()
    }

    fn sql(&self, sql: &str) {
        self.db.conn().execute_batch(sql).unwrap();
    }
}

/// The sub-account error behind a failed preview or commit.
fn sub_accounts(error: import::Error) -> Error {
    match error {
        import::Error::SubAccounts(error) => error,
        other => panic!("not a sub-account error: {other}"),
    }
}

/// The lines a preview commits as new rows.
fn new_lines(preview: &import::ImportPreview) -> impl Iterator<Item = &StatementLine> {
    preview
        .rows()
        .filter(|row| matches!(row.outcome, RowOutcome::New { .. }))
        .filter_map(|row| row.line)
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

/// A Monzo-like pot transfer: the pot's name as merchant, its ID as the
/// description.
fn pot_row(n: u32, d: u32, minor: i64, key: &str, name: Option<&str>) -> StatementLine {
    let mut line = StatementLine::new(day(d), gbp(minor), key);
    line.time = Some(NaiveTime::from_hms_opt(9, 0, n).unwrap());
    line.external_id = Some(format!("tx_{n:04}"));
    line.merchant = name.map(Into::into);
    line.source_type = Some("Pot transfer".into());
    line.source_category = Some("Savings".into());
    line.note = Some("synthetic note".into());
    line.category = Some(crate::category::CategoryKey::new("transfer.savings").unwrap());
    line.sub_account = Some(SubAccountRef {
        kind: SubAccountKind::Pot,
        key: key.into(),
        name: name.map(Into::into),
    });
    line
}

fn card_row(n: u32, d: u32, minor: i64) -> StatementLine {
    let mut line = StatementLine::new(day(d), gbp(minor), "BEAN THERE TESTVILLE GBR");
    line.external_id = Some(format!("tx_{n:04}"));
    line
}

fn statement(lines: impl IntoIterator<Item = StatementLine>) -> Statement {
    let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP).with_preset("uk-monzo");
    for (line, at) in lines.into_iter().zip(2..) {
        statement.push(SourceLocation::Line(at), Ok(line));
    }
    statement
}

/// Into and out of one pot, into a second, and into the first again after
/// the user renamed it at the bank; and a card payment.
fn monzo_like() -> Statement {
    statement([
        pot_row(1, 1, -20_000, RAINY, Some("Rainy Day")),
        card_row(2, 2, -340),
        pot_row(3, 9, 5_000, RAINY, Some("Rainy Day")),
        pot_row(4, 12, -7_500, HOLIDAY, Some("Holiday Fund")),
        pot_row(5, 13, -2_500, RAINY, Some("Emergency Fund")),
    ])
}

/// `(account, batch, date, time, amount, origin, external_id, fingerprint,
/// description, merchant, note, category key, category source, group)` of a
/// transaction.
type Stored = (
    AccountId,
    Option<BatchId>,
    String,
    Option<String>,
    i64,
    String,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<TransferGroupId>,
);

fn stored(fixture: &Fixture, id: TransactionId) -> Stored {
    fixture
        .db
        .conn()
        .query_row(
            "SELECT t.account_id, t.batch_id, t.date, t.time, t.amount_minor, t.origin, \
             t.external_id, t.fingerprint, t.description_raw, t.merchant, t.note, c.key, \
             t.category_source, t.transfer_group_id \
             FROM \"transaction\" t LEFT JOIN category c ON c.id = t.category_id WHERE t.id = ?1",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                    row.get(13)?,
                ))
            },
        )
        .unwrap()
}

#[test]
fn pots_are_created_once_per_key_and_each_row_is_mirrored() {
    let mut fixture = Fixture::new();
    let current = fixture.account("Monzo", AccountKind::Current);

    let imported = fixture.import(current, monzo_like()).unwrap();
    assert_eq!(
        imported.plan,
        MirrorPlan {
            new_pots: vec![
                // Named after its latest row: the user renamed it.
                NewPot {
                    kind: SubAccountKind::Pot,
                    key: RAINY.into(),
                    name: "Emergency Fund".into(),
                },
                NewPot {
                    kind: SubAccountKind::Pot,
                    key: HOLIDAY.into(),
                    name: "Holiday Fund".into(),
                },
            ],
            existing_pots: vec![],
            closed_pots: vec![],
            mirrored_rows: 4,
        }
    );

    let mirrored = &imported.mirrored;
    let rainy = fixture.pot_keyed(current, RAINY).unwrap();
    let holiday = fixture.pot_keyed(current, HOLIDAY).unwrap();
    assert_eq!(mirrored.created_pots, [rainy.id, holiday.id]);
    assert_eq!(mirrored.mirrored_rows(), 4);
    for pot in [&rainy, &holiday] {
        assert_eq!(pot.kind, AccountKind::Pot);
        assert_eq!(pot.parent, Some(current));
        assert_eq!(pot.currency, Currency::GBP);
        assert_eq!(pot.institution.as_deref(), Some("Example Bank"));
        assert_eq!(pot.opening_balance, None);
    }
    assert_eq!(rainy.name, "Emergency Fund");
    assert_eq!(holiday.name, "Holiday Fund");
    assert_eq!(
        pots(&fixture.db, current).unwrap().len(),
        2,
        "one pot per key"
    );

    // Each mirror is the parent row seen from the pot.
    let pot_of = [rainy.id, rainy.id, holiday.id, rainy.id];
    for (row, pot) in mirrored.rows.iter().zip(pot_of) {
        assert_eq!(row.pot, pot);
        let parent = stored(&fixture, row.parent_row);
        let mirror = stored(&fixture, row.mirrored_row);
        assert_eq!(parent.0, current);
        assert_eq!(mirror.0, pot);
        assert_eq!(mirror.1, imported.batch, "same batch");
        assert_eq!(
            (&mirror.2, &mirror.3),
            (&parent.2, &parent.3),
            "same date and time"
        );
        assert_eq!(mirror.4, -parent.4, "opposite amount");
        assert_eq!(
            (parent.5.as_str(), mirror.5.as_str()),
            ("imported", "mirrored")
        );
        assert_eq!((&mirror.6, &mirror.7), (&parent.6, &parent.7), "same keys");
        assert_eq!(
            (&mirror.8, &mirror.9, &mirror.10),
            (&parent.8, &parent.9, &parent.10)
        );
        assert_eq!(
            (mirror.11.as_deref(), mirror.12.as_deref()),
            (Some("transfer.savings"), Some("preset_map")),
            "the parent's category"
        );
        assert_eq!(mirror.13, Some(row.group));
        assert_eq!(parent.13, Some(row.group));

        let group = transfer::group(&fixture.db, row.group).unwrap();
        assert_eq!(group.status, TransferStatus::Confirmed);
        assert_eq!(group.transactions, [row.parent_row, row.mirrored_row]);
    }
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM transfer_group WHERE status = 'confirmed'"),
        4
    );
    // Both directions: into the pot (+200 on it) and out of it (-50).
    let pot_amounts: Vec<i64> = mirrored
        .rows
        .iter()
        .map(|row| stored(&fixture, row.mirrored_row).4)
        .collect();
    assert_eq!(pot_amounts, [20_000, -5_000, 7_500, 2_500]);

    // Detection after mirroring leaves the confirmed pairs alone.
    assert_eq!(imported.transfer_pairs, 0);
}

#[test]
fn a_renamed_pot_is_found_by_its_key() {
    let mut fixture = Fixture::new();
    let current = fixture.account("Monzo", AccountKind::Current);
    fixture
        .import(
            current,
            statement([pot_row(1, 1, -20_000, RAINY, Some("Rainy Day"))]),
        )
        .unwrap();
    let rainy = fixture.pot_keyed(current, RAINY).unwrap();
    // The user renames it in Coffer too.
    fixture.sql(&format!(
        "UPDATE account SET name = 'Rainy Day (joint)' WHERE id = {}",
        rainy.id
    ));

    let next = statement([
        pot_row(2, 3, -1_000, RAINY, Some("Emergency Fund")),
        pot_row(3, 4, -1_000, "pot_unnamed", None),
    ]);
    let imported = fixture.import(current, next).unwrap();
    assert_eq!(
        imported.plan,
        MirrorPlan {
            new_pots: vec![NewPot {
                kind: SubAccountKind::Pot,
                key: "pot_unnamed".into(),
                // No row names it.
                name: "pot_unnamed".into(),
            }],
            existing_pots: vec![rainy.id],
            closed_pots: vec![],
            mirrored_rows: 2,
        }
    );
    assert_eq!(imported.mirrored.rows[0].pot, rainy.id);
    assert_eq!(
        fixture.get(rainy.id).name,
        "Rainy Day (joint)",
        "an existing pot keeps its name"
    );
    assert_eq!(pots(&fixture.db, current).unwrap().len(), 2);
}

#[test]
fn undo_removes_the_mirrored_rows_and_keeps_the_pots() {
    let mut fixture = Fixture::new();
    let current = fixture.account("Monzo", AccountKind::Current);
    let imported = fixture.import(current, monzo_like()).unwrap();
    let batch = imported.batch.unwrap();
    let created = imported.mirrored.created_pots.clone();

    let preview = import::undo_preview(&fixture.db, batch).unwrap();
    assert_eq!((preview.transactions, preview.mirrored), (5, 4));
    // Both sides of each pot pair are the batch's, so nothing is left to
    // dissolve into an ordinary transaction.
    assert_eq!(preview.transfers, 0);
    let undone = import::undo(&mut fixture.db, batch).unwrap();
    assert_eq!(undone.mirrored, 4);
    assert_eq!(fixture.count("SELECT COUNT(*) FROM \"transaction\""), 0);
    assert_eq!(fixture.count("SELECT COUNT(*) FROM transfer_group"), 0);
    let kept: Vec<AccountId> = pots(&fixture.db, current)
        .unwrap()
        .into_iter()
        .map(|pot| pot.account.id)
        .collect();
    assert_eq!(kept, created, "the pots stay");

    // Importing the file again finds them by key and creates none.
    let again = fixture.import(current, monzo_like()).unwrap();
    assert_eq!(again.plan.new_pots, []);
    assert_eq!(again.plan.existing_pots, created);
    assert_eq!(again.mirrored.created_pots, []);
    assert_eq!(again.mirrored.mirrored_rows(), 4);
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM \"transaction\" WHERE origin = 'mirrored'"),
        4
    );
}

#[test]
fn importing_the_same_file_again_mirrors_nothing() {
    let mut fixture = Fixture::new();
    let current = fixture.account("Monzo", AccountKind::Current);
    fixture.import(current, monzo_like()).unwrap();

    let again = fixture.import(current, monzo_like()).unwrap();
    assert_eq!(again.batch, None, "nothing new");
    assert!(again.plan.is_empty());
    assert_eq!(again.plan, MirrorPlan::default());
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM \"transaction\" WHERE origin = 'mirrored'"),
        4
    );
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM account WHERE kind = 'pot'"),
        2
    );

    // An overlapping export: only its new pot row is mirrored.
    let overlapping = statement([
        pot_row(5, 13, -2_500, RAINY, Some("Emergency Fund")),
        pot_row(6, 14, -1_000, RAINY, Some("Emergency Fund")),
    ]);
    let next = fixture.import(current, overlapping).unwrap();
    assert_eq!(next.plan.mirrored_rows, 1);
    assert_eq!(next.mirrored.mirrored_rows(), 1);
}

#[test]
fn mirroring_a_row_twice_is_refused() {
    let mut fixture = Fixture::new();
    let current = fixture.account("Monzo", AccountKind::Current);
    let imported = fixture.import(current, monzo_like()).unwrap();
    let batch = imported.batch.unwrap();
    let first = imported.mirrored.rows[0];
    let parent = fixture.get(current);
    let sub_account = SubAccountRef {
        kind: SubAccountKind::Pot,
        key: RAINY.into(),
        name: None,
    };

    let tx = fixture.db.conn_mut().transaction().unwrap();
    // The pot's source-ID index holds one mirror per parent row.
    let again = mirror(&tx, &parent, batch, &[(first.parent_row, sub_account)]);
    assert!(matches!(again, Err(Error::Database(_))), "{again:?}");
}

#[test]
fn only_the_batch_rows_on_the_parent_are_mirrored() {
    let mut fixture = Fixture::new();
    let current = fixture.account("Monzo", AccountKind::Current);
    let imported = fixture.import(current, monzo_like()).unwrap();
    let batch = imported.batch.unwrap();
    let mirrored_row = imported.mirrored.rows[0].mirrored_row;
    let parent = fixture.get(current);
    let sub_account = SubAccountRef {
        kind: SubAccountKind::Pot,
        key: RAINY.into(),
        name: None,
    };

    let tx = fixture.db.conn_mut().transaction().unwrap();
    for (id, in_batch) in [
        (mirrored_row, batch),
        (TransactionId::new(999), batch),
        (imported.mirrored.rows[0].parent_row, BatchId::new(999)),
    ] {
        assert!(
            matches!(
                mirror(&tx, &parent, in_batch, &[(id, sub_account.clone())]),
                Err(Error::NotInBatch { transaction, batch })
                    if transaction == id && batch == in_batch
            ),
            "{id}"
        );
    }
}

#[test]
fn a_parent_that_cant_have_pots_is_an_error() {
    let mut fixture = Fixture::new();
    let card = fixture.account("Card", AccountKind::CreditCard);
    let cash = fixture.account("Wallet", AccountKind::Cash);
    let current = fixture.account("Monzo", AccountKind::Current);
    let savings = fixture.account("Savings", AccountKind::Savings);
    let mut pot = NewAccount::new("Pot", AccountKind::Pot, Currency::GBP);
    pot.parent = Some(current);
    let pot = account::create(&mut fixture.db, &pot).unwrap();

    for (account, kind) in [(card, "credit_card"), (cash, "cash"), (pot, "pot")] {
        assert!(
            matches!(
                fixture.plan(account, monzo_like()),
                Err(Error::CannotHavePots { kind: found }) if found == kind
            ),
            "{kind}"
        );
        let error = fixture.import(account, monzo_like()).err();
        assert!(
            matches!(&error, Some(Error::CannotHavePots { kind: found }) if *found == kind),
            "{kind}: {error:?}"
        );
        assert_eq!(
            error.unwrap().to_string(),
            format!(
                "the file has transfers to and from pots, but {kind} accounts can't have pots; \
                 import it into a current or savings account"
            )
        );
    }
    // Without pot rows, any account that holds transactions is fine.
    let plain = statement([card_row(1, 1, -340)]);
    assert_eq!(fixture.plan(card, plain).unwrap(), MirrorPlan::default());

    // A savings account can have pots.
    let imported = fixture.import(savings, monzo_like()).unwrap();
    assert_eq!(imported.mirrored.created_pots.len(), 2);
}

#[test]
fn keys_are_unique_per_parent_only() {
    let mut fixture = Fixture::new();
    let personal = fixture.account("Monzo", AccountKind::Current);
    let joint = fixture.account("Monzo joint", AccountKind::Current);
    fixture.import(personal, monzo_like()).unwrap();

    let imported = fixture.import(joint, monzo_like()).unwrap();
    assert_eq!(
        imported.plan.new_pots.len(),
        2,
        "another parent's pots don't match"
    );
    assert_eq!(imported.mirrored.created_pots.len(), 2);
    assert_ne!(
        fixture.pot_keyed(personal, RAINY).unwrap().id,
        fixture.pot_keyed(joint, RAINY).unwrap().id
    );
}

#[test]
fn a_closed_pot_still_gets_its_rows_and_a_locked_one_refuses() {
    let mut fixture = Fixture::new();
    let current = fixture.account("Monzo", AccountKind::Current);
    let first = fixture
        .import(
            current,
            statement([pot_row(1, 1, -20_000, RAINY, Some("Rainy Day"))]),
        )
        .unwrap();
    let rainy = fixture.pot_keyed(current, RAINY).unwrap().id;
    fixture.sql(&format!(
        "UPDATE account SET closed_on = '2026-10-05' WHERE id = {rainy}"
    ));

    let next = statement([pot_row(2, 3, 20_000, RAINY, Some("Rainy Day"))]);
    let imported = fixture.import(current, next).unwrap();
    assert_eq!(imported.plan.existing_pots, [rainy]);
    assert_eq!(imported.plan.closed_pots, [rainy]);
    assert_eq!(imported.mirrored.rows[0].pot, rainy);

    // Locked through the 5th: a row on the 5th is refused, before anything
    // is written, by the plan and by the mirror.
    fixture.sql(&format!(
        "UPDATE account SET reconciled_through = '2026-10-05' WHERE id = {rainy}"
    ));
    let locked = || {
        statement([
            pot_row(3, 5, -100, RAINY, None),
            pot_row(4, 6, -100, RAINY, None),
        ])
    };
    let expected = |error: &Error| matches!(error, Error::PotLocked { pot, through } if *pot == rainy && *through == day(5));
    assert!(expected(&fixture.plan(current, locked()).unwrap_err()));
    let before = fixture.count("SELECT COUNT(*) FROM \"transaction\" WHERE origin = 'mirrored'");
    assert!(expected(&fixture.import(current, locked()).err().unwrap()));
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM \"transaction\" WHERE origin = 'mirrored'"),
        before,
        "the failed mirror wrote nothing"
    );
    // `mirror` checks the lock itself, before writing anything.
    let parent = fixture.get(current);
    let tx = fixture.db.conn_mut().transaction().unwrap();
    let again = mirror(
        &tx,
        &parent,
        first.batch.unwrap(),
        &[(
            first.mirrored.rows[0].parent_row,
            SubAccountRef {
                kind: SubAccountKind::Pot,
                key: RAINY.into(),
                name: None,
            },
        )],
    );
    assert!(expected(&again.unwrap_err()));
    drop(tx);
    // After the lock, fine.
    let after = statement([pot_row(5, 6, -100, RAINY, None)]);
    assert_eq!(fixture.plan(current, after).unwrap().mirrored_rows, 1);
}

#[test]
fn nothing_to_mirror_does_nothing() {
    let mut fixture = Fixture::new();
    let card = fixture.account("Card", AccountKind::CreditCard);
    let parent = fixture.get(card);
    let tx = fixture.db.conn_mut().transaction().unwrap();
    assert_eq!(
        mirror(&tx, &parent, BatchId::new(1), &[]).unwrap(),
        MirrorSummary::default()
    );
    assert_eq!(plan(&tx, &parent, []).unwrap(), MirrorPlan::default());
}

#[test]
fn the_name_comes_from_the_latest_dated_row() {
    let named = |date: u32, name: Option<&str>| {
        (
            day(date),
            SubAccountRef {
                kind: SubAccountKind::Pot,
                key: RAINY.into(),
                name: name.map(Into::into),
            },
        )
    };
    // Newest first, as some exports are; blank names don't count.
    let items = [
        named(9, Some("  ")),
        named(8, Some("Newest")),
        named(8, Some("Same day, later in the file")),
        named(1, Some("Oldest")),
        named(10, None),
    ];
    let keys = by_key(items.iter().map(|(date, sub_account)| (*date, sub_account)));
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].name, "Same day, later in the file");
    assert_eq!(keys[0].earliest, day(1));
    assert_eq!(keys[0].rows, 5);
}

// The confirmed pair -------------------------------------------------------------

#[test]
fn a_confirmed_pair_must_be_a_pair() {
    let mut fixture = Fixture::new();
    let current = fixture.account("Monzo", AccountKind::Current);
    let savings = fixture.account("Savings", AccountKind::Savings);
    let insert = |fixture: &Fixture, account: AccountId, minor: i64| -> TransactionId {
        fixture
            .db
            .conn()
            .execute(
                "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
                 description_raw, description_normalized, origin) \
                 VALUES (?1, '2026-10-01', ?2, 'GBP', 'x', 'X', 'manual')",
                params![account, minor],
            )
            .unwrap();
        TransactionId::new(fixture.db.conn().last_insert_rowid())
    };
    let out = insert(&fixture, current, -500);
    let into = insert(&fixture, savings, 500);
    let same_account = insert(&fixture, current, 500);
    let wrong_amount = insert(&fixture, savings, 400);

    let conn = fixture.db.conn();
    for (a, b) in [(out, same_account), (out, wrong_amount)] {
        assert!(matches!(
            transfer::record_confirmed_pair(conn, a, b),
            Err(transfer::Error::NotAPair(x, y)) if (x, y) == (a, b)
        ));
    }
    assert!(matches!(
        transfer::record_confirmed_pair(conn, out, TransactionId::new(999)),
        Err(transfer::Error::TransactionNotFound(_))
    ));

    // Either order.
    let group = transfer::record_confirmed_pair(conn, into, out).unwrap();
    let stored = transfer::group(&fixture.db, group).unwrap();
    assert_eq!(stored.status, TransferStatus::Confirmed);
    assert_eq!(stored.transactions, [out, into]);

    let again = insert(&fixture, savings, 500);
    assert!(matches!(
        transfer::record_confirmed_pair(fixture.db.conn(), out, again),
        Err(transfer::Error::AlreadyGrouped(id)) if id == out
    ));
    // Detection skips grouped rows.
    let pairs = transfer::detect(
        fixture.db.conn(),
        &[out, into, again],
        TransferWindow::DEFAULT,
    )
    .unwrap();
    assert_eq!(pairs, []);
}

// The schema (V4) ----------------------------------------------------------------

#[test]
fn only_pots_have_a_key_unique_under_their_parent_and_fixed() {
    let fixture = Fixture::new();
    let conn = fixture.db.conn();
    let add = |parent: Option<i64>, kind: &str, key: Option<&str>| {
        conn.execute(
            "INSERT INTO account (parent_id, name, kind, currency, sub_account_key) \
             VALUES (?1, 'x', ?2, 'GBP', ?3)",
            params![parent, kind, key],
        )
        .map(|_| conn.last_insert_rowid())
    };
    let current = add(None, "current", None).unwrap();
    let other = add(None, "current", None).unwrap();
    assert!(add(None, "current", Some("pot_1")).is_err(), "only pots");
    assert!(add(Some(current), "pot", Some("")).is_err(), "not empty");
    let pot = add(Some(current), "pot", Some("pot_1")).unwrap();
    assert!(
        add(Some(current), "pot", Some("pot_1")).is_err(),
        "unique per parent"
    );
    add(Some(other), "pot", Some("pot_1")).unwrap();
    add(Some(current), "pot", None).unwrap();
    add(Some(current), "pot", None).unwrap();

    assert!(
        conn.execute(
            "UPDATE account SET sub_account_key = 'pot_2' WHERE id = ?1",
            [pot]
        )
        .is_err(),
        "a key never changes"
    );
    assert!(
        conn.execute(
            "UPDATE account SET sub_account_key = NULL WHERE id = ?1",
            [pot]
        )
        .is_err()
    );
    conn.execute("UPDATE account SET name = 'Renamed' WHERE id = ?1", [pot])
        .unwrap();
}
