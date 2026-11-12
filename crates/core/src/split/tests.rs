//! Tests for split transactions: validation on its own, then against a real
//! database.

use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount};
use crate::id::AccountId;
use crate::transfer::{self, TransferWindow};

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn key(key: &str) -> CategoryKey {
    CategoryKey::new(key).unwrap()
}

fn line(minor: i64, category: &str) -> SplitLine {
    SplitLine::new(gbp(minor), Some(key(category)))
}

// Validation ------------------------------------------------------------------

#[test]
fn lines_that_sum_to_the_amount_are_a_split() {
    validate(
        gbp(-1234),
        &[line(-1000, "food.groceries"), line(-234, "home")],
    )
    .unwrap();
    // A refund splits into amounts back, and a line may be uncategorised.
    validate(
        gbp(500),
        &[line(300, "food"), SplitLine::new(gbp(200), None)],
    )
    .unwrap();
    // Any number of lines from two.
    validate(
        gbp(-3),
        &[line(-1, "food"), line(-1, "food"), line(-1, "home")],
    )
    .unwrap();
}

#[test]
fn a_split_has_two_lines_or_more() {
    assert!(matches!(
        validate(gbp(-500), &[]),
        Err(Error::TooFewLines(0))
    ));
    assert!(matches!(
        validate(gbp(-500), &[line(-500, "food")]),
        Err(Error::TooFewLines(1))
    ));
}

#[test]
fn a_zero_transaction_isnt_split() {
    assert!(matches!(
        validate(gbp(0), &[line(-1, "food"), line(1, "home")]),
        Err(Error::ZeroAmount)
    ));
}

#[test]
fn every_line_has_the_transactions_sign() {
    assert!(matches!(
        validate(gbp(-500), &[line(-500, "food"), line(0, "home")]),
        Err(Error::ZeroLine(2))
    ));
    // Even when the sum works out.
    assert!(matches!(
        validate(gbp(-500), &[line(-700, "food"), line(200, "home")]),
        Err(Error::WrongSign(2))
    ));
    assert!(matches!(
        validate(gbp(500), &[line(-100, "food"), line(600, "home")]),
        Err(Error::WrongSign(1))
    ));
}

#[test]
fn every_line_is_in_the_transactions_currency() {
    let euros = SplitLine::new(Money::new(-200, Currency::EUR), None);
    assert!(matches!(
        validate(gbp(-500), &[line(-300, "food"), euros]),
        Err(Error::CurrencyMismatch {
            line: 2,
            expected: Currency::GBP,
            found: Currency::EUR,
        })
    ));

    let mut share = line(-300, "food");
    share.my_share = Some(Money::new(-100, Currency::EUR));
    assert!(matches!(
        validate(gbp(-500), &[share, line(-200, "home")]),
        Err(Error::CurrencyMismatch { line: 1, .. })
    ));
}

#[test]
fn the_lines_sum_exactly_to_the_amount() {
    match validate(gbp(-500), &[line(-300, "food"), line(-199, "home")]) {
        Err(Error::SumMismatch { expected, found }) => {
            assert_eq!(expected, gbp(-500));
            assert_eq!(found, gbp(-499));
        }
        other => panic!("expected a sum mismatch, got {other:?}"),
    }
    assert!(matches!(
        validate(gbp(-500), &[line(-300, "food"), line(-201, "home")]),
        Err(Error::SumMismatch { .. })
    ));
    assert!(matches!(
        validate(
            gbp(i64::MAX),
            &[line(i64::MAX, "food"), line(i64::MAX, "home")]
        ),
        Err(Error::Overflow)
    ));
}

#[test]
fn a_lines_share_is_part_of_it() {
    let with_share = |minor: i64| {
        let mut first = line(-300, "food");
        first.my_share = Some(gbp(minor));
        validate(gbp(-500), &[first, line(-200, "home")])
    };
    with_share(-150).unwrap();
    with_share(0).unwrap();
    with_share(-300).unwrap();
    assert!(matches!(with_share(150), Err(Error::ShareOutsideLine(1))));
    assert!(matches!(with_share(-301), Err(Error::ShareOutsideLine(1))));

    let mut refund = SplitLine::new(gbp(300), None);
    refund.my_share = Some(gbp(301));
    assert!(matches!(
        validate(gbp(500), &[SplitLine::new(gbp(200), None), refund]),
        Err(Error::ShareOutsideLine(2))
    ));
}

#[test]
fn a_share_label_isnt_blank() {
    let mut labelled = line(-300, "food");
    labelled.share_label = Some(" \t".to_owned());
    assert!(matches!(
        validate(gbp(-500), &[line(-200, "home"), labelled]),
        Err(Error::BlankShareLabel(2))
    ));
}

/// Errors name lines by number, never by the user's text.
#[test]
fn errors_dont_quote_the_lines() {
    let mut private = line(-300, "health.pharmacy");
    private.share_label = Some("Sam Secret".to_owned());
    private.user_note = Some("Prescription for Sam".to_owned());
    private.my_share = Some(gbp(-301));
    let message = validate(gbp(-500), &[private, line(-200, "home")])
        .unwrap_err()
        .to_string();
    for text in ["Sam", "Prescription", "pharmacy", "health"] {
        assert!(!message.contains(text), "{message}");
    }
}

// The database ----------------------------------------------------------------

struct Fixture {
    db: Database,
    account: AccountId,
    _dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        Fixture {
            db,
            account,
            _dir: dir,
        }
    }

    fn conn(&self) -> &Connection {
        self.db.conn()
    }

    /// Inserts a manual transaction of `minor` GBP on `account`.
    fn manual_on(&self, account: AccountId, minor: i64) -> TransactionId {
        self.conn()
            .execute(
                "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
                 description_raw, description_normalized, origin) \
                 VALUES (?1, '2026-11-03', ?2, 'GBP', 'Shop', 'SHOP', 'manual')",
                params![account, minor],
            )
            .unwrap();
        TransactionId::new(self.conn().last_insert_rowid())
    }

    fn manual(&self, minor: i64) -> TransactionId {
        self.manual_on(self.account, minor)
    }

    /// Categorises `transaction` as a rule would, with a share.
    fn categorise(&self, transaction: TransactionId, category: &str) {
        self.conn()
            .execute(
                "INSERT INTO rule (priority, match_field, match_kind, pattern) \
                 VALUES (1, 'description', 'contains', 'shop')",
                [],
            )
            .unwrap();
        let rule = self.conn().last_insert_rowid();
        self.conn()
            .execute(
                "UPDATE \"transaction\" SET \
                 category_id = (SELECT id FROM category WHERE key = ?1), \
                 category_source = 'rule', rule_id = ?2, my_share_minor = amount_minor, \
                 share_label = 'Sam' WHERE id = ?3",
                params![category, rule, transaction],
            )
            .unwrap();
    }

    /// `transaction`'s category key, category source, rule, share and label.
    #[allow(clippy::type_complexity)]
    fn own_fields(
        &self,
        transaction: TransactionId,
    ) -> (
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<String>,
    ) {
        self.conn()
            .query_row(
                "SELECT c.key, t.category_source, t.rule_id, t.my_share_minor, t.share_label \
                 FROM \"transaction\" t LEFT JOIN category c ON c.id = t.category_id \
                 WHERE t.id = ?1",
                [transaction],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap()
    }

    /// The ledger rows of `transaction`: `(line_no, amount, category key)`.
    fn ledger(&self, transaction: TransactionId) -> Vec<(Option<i64>, i64, Option<String>)> {
        let mut statement = self
            .conn()
            .prepare(
                "SELECT l.line_no, l.amount_minor, c.key FROM ledger_line l \
                 LEFT JOIN category c ON c.id = l.category_id \
                 WHERE l.transaction_id = ?1 ORDER BY l.line_no",
            )
            .unwrap();
        statement
            .query_map([transaction], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }
}

#[test]
fn set_stores_the_lines_and_clears_the_transactions_own_category() {
    let mut fixture = Fixture::new();
    let id = fixture.manual(-1234);
    fixture.categorise(id, "food");

    let mut shared = line(-1000, "food.groceries");
    shared.my_share = Some(gbp(-500));
    shared.share_label = Some("Sam".to_owned());
    shared.user_note = Some("Split with Sam".to_owned());
    let ids = set(
        &mut fixture.db,
        id,
        &[shared, SplitLine::new(gbp(-234), None)],
    )
    .unwrap();

    assert_eq!(ids.len(), 2);
    assert_eq!(fixture.own_fields(id), (None, None, None, None, None));
    let stored = lines(&fixture.db, id).unwrap();
    assert_eq!(
        stored,
        [
            StoredSplitLine {
                id: ids[0],
                transaction: id,
                line_no: 1,
                amount: gbp(-1000),
                category: Some(key("food.groceries")),
                my_share: Some(gbp(-500)),
                share_label: Some("Sam".to_owned()),
                user_note: Some("Split with Sam".to_owned()),
            },
            StoredSplitLine {
                id: ids[1],
                transaction: id,
                line_no: 2,
                amount: gbp(-234),
                category: None,
                my_share: None,
                share_label: None,
                user_note: None,
            },
        ]
    );
    assert_eq!(
        fixture.ledger(id),
        [
            (Some(1), -1000, Some("food.groceries".to_owned())),
            (Some(2), -234, None),
        ]
    );
}

#[test]
fn set_replaces_an_earlier_split() {
    let mut fixture = Fixture::new();
    let id = fixture.manual(-500);
    let first = set(
        &mut fixture.db,
        id,
        &[line(-300, "food"), line(-200, "home")],
    )
    .unwrap();
    let second = set(
        &mut fixture.db,
        id,
        &[line(-100, "travel"), line(-100, "food"), line(-300, "home")],
    )
    .unwrap();

    assert!(second.iter().all(|id| !first.contains(id)));
    assert_eq!(
        fixture.ledger(id),
        [
            (Some(1), -100, Some("travel".to_owned())),
            (Some(2), -100, Some("food".to_owned())),
            (Some(3), -300, Some("home".to_owned())),
        ]
    );
}

#[test]
fn an_invalid_split_changes_nothing() {
    let mut fixture = Fixture::new();
    let id = fixture.manual(-500);
    fixture.categorise(id, "food");
    let before = fixture.own_fields(id);

    let missing = SplitLine::new(gbp(-200), Some(key("no_such.category")));
    assert!(matches!(
        set(&mut fixture.db, id, &[line(-300, "home"), missing]),
        Err(Error::UnknownCategory(2))
    ));
    assert!(matches!(
        set(
            &mut fixture.db,
            id,
            &[line(-300, "home"), line(-100, "food")]
        ),
        Err(Error::SumMismatch { .. })
    ));
    assert_eq!(fixture.own_fields(id), before);
    assert!(lines(&fixture.db, id).unwrap().is_empty());
    assert_eq!(fixture.ledger(id), [(None, -500, Some("food".to_owned()))]);

    // An invalid replacement keeps the split there.
    set(
        &mut fixture.db,
        id,
        &[line(-300, "food"), line(-200, "home")],
    )
    .unwrap();
    assert!(matches!(
        set(&mut fixture.db, id, &[line(-500, "food")]),
        Err(Error::TooFewLines(1))
    ));
    assert_eq!(lines(&fixture.db, id).unwrap().len(), 2);
}

#[test]
fn a_missing_transaction_is_an_error() {
    let mut fixture = Fixture::new();
    let missing = TransactionId::new(999);
    assert!(matches!(
        set(
            &mut fixture.db,
            missing,
            &[line(-300, "food"), line(-200, "home")]
        ),
        Err(Error::TransactionNotFound(id)) if id == missing
    ));
    assert!(matches!(
        lines(&fixture.db, missing),
        Err(Error::TransactionNotFound(_))
    ));
    assert!(matches!(
        remove(&mut fixture.db, missing),
        Err(Error::TransactionNotFound(_))
    ));
}

#[test]
fn removing_a_split_leaves_the_transaction_uncategorised() {
    let mut fixture = Fixture::new();
    let id = fixture.manual(-500);
    fixture.categorise(id, "food");
    set(
        &mut fixture.db,
        id,
        &[line(-300, "food"), line(-200, "home")],
    )
    .unwrap();

    assert_eq!(remove(&mut fixture.db, id).unwrap(), 2);
    assert!(lines(&fixture.db, id).unwrap().is_empty());
    assert_eq!(fixture.own_fields(id), (None, None, None, None, None));
    assert_eq!(fixture.ledger(id), [(None, -500, None)]);
    assert!(matches!(
        remove(&mut fixture.db, id),
        Err(Error::NotSplit(_))
    ));
}

/// Exclusion is about whether the row counts; its lines carry the reason.
#[test]
fn an_excluded_transaction_can_be_split() {
    let mut fixture = Fixture::new();
    let id = fixture.manual(-500);
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET excluded_reason = 'disputed', \
             excluded_at = '2026-11-12T10:00:00.000Z' WHERE id = ?1",
            [id],
        )
        .unwrap();

    set(
        &mut fixture.db,
        id,
        &[line(-300, "food"), line(-200, "home")],
    )
    .unwrap();
    let excluded: i64 = fixture
        .conn()
        .query_row(
            "SELECT count(*) FROM ledger_line \
             WHERE transaction_id = ?1 AND excluded_reason = 'disputed'",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(excluded, 2);
}

#[test]
fn a_transfer_cant_be_split() {
    let mut fixture = Fixture::new();
    let savings = account::create(
        &mut fixture.db,
        &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
    )
    .unwrap();
    let out = fixture.manual(-500);
    let back = fixture.manual_on(savings, 500);
    let pairs = transfer::detect(fixture.conn(), &[out], TransferWindow::DEFAULT).unwrap();
    assert_eq!(pairs.len(), 1);
    let group = pairs[0].group;
    let split = [line(-300, "food"), line(-200, "home")];

    assert!(matches!(
        set(&mut fixture.db, out, &split),
        Err(Error::InTransfer(id)) if id == out
    ));
    fixture
        .conn()
        .execute(
            "UPDATE transfer_group SET status = 'confirmed' WHERE id = ?1",
            [group],
        )
        .unwrap();
    assert!(matches!(
        set(&mut fixture.db, out, &split),
        Err(Error::InTransfer(_))
    ));

    // A rejected pair is two ordinary transactions.
    fixture
        .conn()
        .execute(
            "UPDATE transfer_group SET status = 'rejected' WHERE id = ?1",
            [group],
        )
        .unwrap();
    set(&mut fixture.db, out, &split).unwrap();

    // Detection doesn't suggest a split row: the schema would refuse it.
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET transfer_group_id = NULL WHERE transfer_group_id = ?1",
            [group],
        )
        .unwrap();
    let pairs = transfer::detect(fixture.conn(), &[out, back], TransferWindow::DEFAULT).unwrap();
    assert!(pairs.is_empty());
}

/// A manual row's amount is editable (M1.22), but not while its lines must
/// sum to it.
#[test]
fn a_split_transactions_amount_cant_change() {
    let mut fixture = Fixture::new();
    let id = fixture.manual(-500);
    set(
        &mut fixture.db,
        id,
        &[line(-300, "food"), line(-200, "home")],
    )
    .unwrap();
    let edit = |db: &Database| {
        db.conn().execute(
            "UPDATE \"transaction\" SET amount_minor = -700 WHERE id = ?1",
            [id],
        )
    };

    let refused = edit(&fixture.db).unwrap_err();
    assert_eq!(
        refused.sqlite_error_code(),
        Some(rusqlite::ErrorCode::ConstraintViolation)
    );
    remove(&mut fixture.db, id).unwrap();
    edit(&fixture.db).unwrap();
}

#[test]
fn the_ledger_sums_to_the_transactions() {
    let mut fixture = Fixture::new();
    let plain = fixture.manual(-250);
    let split = fixture.manual(-1000);
    let refund = fixture.manual(400);
    set(
        &mut fixture.db,
        split,
        &[line(-600, "food"), line(-399, "home"), line(-1, "other")],
    )
    .unwrap();
    set(
        &mut fixture.db,
        refund,
        &[line(100, "food"), line(300, "home")],
    )
    .unwrap();

    assert_eq!(fixture.ledger(plain).len(), 1);
    assert_eq!(fixture.ledger(split).len(), 3);
    assert_eq!(fixture.ledger(refund).len(), 2);
    let per_transaction: Vec<(i64, i64, i64)> = {
        let mut statement = fixture
            .conn()
            .prepare(
                "SELECT t.id, t.amount_minor, \
                 (SELECT sum(amount_minor) FROM ledger_line l WHERE l.transaction_id = t.id) \
                 FROM \"transaction\" t ORDER BY t.id",
            )
            .unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(per_transaction.len(), 3);
    for (id, amount, ledger) in per_transaction {
        assert_eq!(amount, ledger, "transaction {id}");
    }
    let food: i64 = fixture
        .conn()
        .query_row(
            "SELECT sum(l.amount_minor) FROM ledger_line l \
             JOIN category c ON c.id = l.category_id WHERE c.key = 'food'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(food, -500);
}

#[test]
fn clear_batch_lets_an_import_be_deleted() {
    let mut fixture = Fixture::new();
    fixture
        .conn()
        .execute(
            "INSERT INTO import_batch (account_id, source_file, format) \
             VALUES (?1, 'statement.csv', 'csv')",
            [fixture.account],
        )
        .unwrap();
    let batch = BatchId::new(fixture.conn().last_insert_rowid());
    fixture
        .conn()
        .execute(
            "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
             description_raw, description_normalized, fingerprint, origin) \
             VALUES (?1, ?2, '2026-11-02', -500, 'GBP', 'Shop', 'SHOP', 'fp-1', 'imported')",
            params![fixture.account, batch],
        )
        .unwrap();
    let imported = TransactionId::new(fixture.conn().last_insert_rowid());
    let manual = fixture.manual(-500);
    let split = [line(-300, "food"), line(-200, "home")];
    set(&mut fixture.db, imported, &split).unwrap();
    set(&mut fixture.db, manual, &split).unwrap();

    let delete = "DELETE FROM \"transaction\" WHERE batch_id = ?1";
    assert!(fixture.conn().execute(delete, [batch]).is_err());
    assert_eq!(clear_batch(fixture.conn(), batch).unwrap(), 2);
    assert_eq!(fixture.conn().execute(delete, [batch]).unwrap(), 1);
    // Another transaction's split stays.
    assert_eq!(lines(&fixture.db, manual).unwrap().len(), 2);

    assert_eq!(clear(fixture.conn(), manual).unwrap(), 2);
    assert_eq!(clear(fixture.conn(), manual).unwrap(), 0);
}
