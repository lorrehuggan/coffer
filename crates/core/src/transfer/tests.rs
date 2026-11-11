//! Tests for transfer-pair detection against a real database.

use chrono::{NaiveDate, NaiveTime};
use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{AccountKind, NewAccount};
use crate::import::{self, Committed, Confirmations};
use crate::statement::{SourceFormat, SourceLocation, Statement, StatementLine};

struct Fixture {
    db: Database,
    _dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("coffer.db")).unwrap();
        Fixture { db, _dir: dir }
    }

    fn account(&mut self, kind: AccountKind) -> AccountId {
        self.account_in(kind, Currency::GBP)
    }

    fn account_in(&mut self, kind: AccountKind, currency: Currency) -> AccountId {
        account::create(&mut self.db, &NewAccount::new("Account", kind, currency)).unwrap()
    }

    fn pot(&mut self, parent: AccountId) -> AccountId {
        let mut pot = NewAccount::new("Pot", AccountKind::Pot, Currency::GBP);
        pot.parent = Some(parent);
        account::create(&mut self.db, &pot).unwrap()
    }

    /// Imports `rows` (`(day, minor)`) into `account` as one CSV batch.
    fn import(&mut self, account: AccountId, rows: &[(u32, i64)]) -> BatchId {
        let lines = rows
            .iter()
            .enumerate()
            .map(|(index, &(d, minor))| {
                StatementLine::new(day(d), money(minor), format!("Row {index}"))
            })
            .collect();
        self.import_lines(account, lines)
    }

    fn import_lines(&mut self, account: AccountId, lines: Vec<StatementLine>) -> BatchId {
        let currency = account::get(&self.db, account).unwrap().currency;
        let mut statement = Statement::new(SourceFormat::Csv, currency);
        for (index, line) in lines.into_iter().enumerate() {
            let mut line = line;
            line.amount = Money::new(line.amount.minor(), currency);
            statement.push(SourceLocation::Line(index as u64 + 2), Ok(line));
        }
        let preview = import::preview(&self.db, account, "statement.csv", statement).unwrap();
        match import::commit(&mut self.db, &preview, Confirmations::default()).unwrap() {
            Committed::Batch(summary) => summary.batch,
            Committed::NothingNew => panic!("expected a new batch"),
        }
    }

    /// A manual transaction (no batch), in the account's currency.
    fn manual(&mut self, account: AccountId, d: u32, minor: i64) -> TransactionId {
        self.manual_at(account, d, None, minor)
    }

    fn manual_at(
        &mut self,
        account: AccountId,
        d: u32,
        time: Option<&str>,
        minor: i64,
    ) -> TransactionId {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO \"transaction\" (account_id, date, time, amount_minor, currency, \
             description_raw, description_normalized, origin) \
             SELECT id, ?2, ?3, ?4, currency, 'Manual', 'MANUAL', 'manual' \
             FROM account WHERE id = ?1",
            params![account, date_key(day(d)), time, minor],
        )
        .unwrap();
        TransactionId::new(conn.last_insert_rowid())
    }

    /// The batch's transactions, by ID.
    fn rows(&self, batch: BatchId) -> Vec<TransactionId> {
        let mut statement = self
            .db
            .conn()
            .prepare("SELECT id FROM \"transaction\" WHERE batch_id = ?1 ORDER BY id")
            .unwrap();
        statement
            .query_map([batch], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn detect(&mut self, batch: BatchId, window: TransferWindow) -> Vec<SuggestedPair> {
        suggest_for_batch(&mut self.db, batch, window).unwrap()
    }

    /// The pairs as `(outflow, inflow)`.
    fn detect_ids(&mut self, batch: BatchId) -> Vec<(TransactionId, TransactionId)> {
        self.detect(batch, TransferWindow::DEFAULT)
            .into_iter()
            .map(|pair| (pair.outflow, pair.inflow))
            .collect()
    }

    fn group_id(&self, transaction: TransactionId) -> Option<TransferGroupId> {
        self.db
            .conn()
            .query_row(
                "SELECT transfer_group_id FROM \"transaction\" WHERE id = ?1",
                [transaction],
                |row| row.get(0),
            )
            .unwrap()
    }

    /// Puts `transaction` in a new group with `status`, as the user's
    /// confirm or reject would.
    fn put_in_group(&self, transaction: TransactionId, status: TransferStatus) {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO transfer_group (status) VALUES (?1)",
            [status.as_str()],
        )
        .unwrap();
        let group = conn.last_insert_rowid();
        conn.execute(
            "UPDATE \"transaction\" SET transfer_group_id = ?1 WHERE id = ?2",
            params![group, transaction],
        )
        .unwrap();
    }

    fn count(&self, sql: &str) -> i64 {
        self.db.conn().query_row(sql, [], |row| row.get(0)).unwrap()
    }
}

/// Day `d` counted from 1 October 2026, so a day past the 31st runs on
/// into November.
fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 1).unwrap() + chrono::Days::new(u64::from(d) - 1)
}

fn money(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn window(days: u32) -> TransferWindow {
    TransferWindow::new(days).unwrap()
}

// The window ----------------------------------------------------------------

#[test]
fn window_is_three_days_by_default_and_at_most_a_month() {
    assert_eq!(TransferWindow::DEFAULT.days(), 3);
    assert_eq!(TransferWindow::default(), TransferWindow::DEFAULT);
    assert_eq!(window(0).days(), 0);
    assert_eq!(window(TransferWindow::MAX_DAYS).days(), 31);
    assert!(matches!(
        TransferWindow::new(32),
        Err(Error::WindowTooLong(32))
    ));
}

#[test]
fn status_keys_match_the_schema() {
    let fx = Fixture::new();
    for status in TransferStatus::ALL {
        fx.db
            .conn()
            .execute(
                "INSERT INTO transfer_group (status) VALUES (?1)",
                [status.as_str()],
            )
            .unwrap();
        assert_eq!(TransferStatus::from_key(status.as_str()), Some(status));
    }
    assert!(
        fx.db
            .conn()
            .execute("INSERT INTO transfer_group (status) VALUES ('other')", [])
            .is_err()
    );
}

// Pairing -------------------------------------------------------------------

#[test]
fn pairs_opposite_amounts_across_two_accounts() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let batch = fx.import(current, &[(10, -5000), (10, -1299)]);
    let out = fx.rows(batch)[0];
    let arrived = fx.manual(savings, 10, 5000);
    // The user already filed the savings side; detection leaves it.
    fx.db
        .conn()
        .execute(
            "UPDATE \"transaction\" SET category_source = 'user', category_id = \
             (SELECT id FROM category WHERE key = 'transfer.savings') WHERE id = ?1",
            [arrived],
        )
        .unwrap();

    let pairs = fx.detect(batch, TransferWindow::DEFAULT);

    assert_eq!(pairs.len(), 1);
    let pair = pairs[0];
    assert_eq!((pair.outflow, pair.inflow), (out, arrived));
    assert_eq!(pair.amount, money(5000));
    assert_eq!(fx.group_id(out), Some(pair.group));
    assert_eq!(fx.group_id(arrived), Some(pair.group));
    assert_eq!(
        group(&fx.db, pair.group).unwrap(),
        TransferGroup {
            id: pair.group,
            status: TransferStatus::Suggested,
            transactions: vec![out, arrived],
        }
    );
    assert_eq!(group_of(&fx.db, arrived).unwrap().unwrap().id, pair.group);
    assert_eq!(group_of(&fx.db, fx.rows(batch)[1]).unwrap(), None);
    // Categories are untouched.
    assert_eq!(
        fx.count(
            "SELECT COUNT(*) FROM \"transaction\" WHERE category_id IS NOT NULL \
             AND category_source = 'user'"
        ),
        1
    );
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM \"transaction\" WHERE category_id IS NOT NULL"),
        1
    );
}

#[test]
fn pairs_a_pot_with_its_parent() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let pot = fx.pot(current);
    let batch = fx.import(current, &[(10, -2500)]);
    let into_pot = fx.manual(pot, 10, 2500);

    assert_eq!(fx.detect_ids(batch), vec![(fx.rows(batch)[0], into_pot)]);
}

#[test]
fn detect_takes_any_set_of_transactions() {
    // Both sides among the transactions passed in, in any order and with
    // repeats: pairs found from either side are the same pair. (A batch's
    // own rows pair the same way: M1.17's mirrored pot rows.)
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let out_batch = fx.import(current, &[(10, -700)]);
    let out = fx.rows(out_batch)[0];
    let arrived_batch = fx.import(savings, &[(11, 700)]);
    let arrived = fx.rows(arrived_batch)[0];

    let tx = fx.db.conn_mut().transaction().unwrap();
    let pairs = detect(&tx, &[arrived, out, arrived], TransferWindow::DEFAULT).unwrap();
    tx.commit().unwrap();

    assert_eq!(pairs.len(), 1);
    assert_eq!((pairs[0].outflow, pairs[0].inflow), (out, arrived));
}

#[test]
fn same_account_is_not_a_transfer() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let batch = fx.import(current, &[(10, -5000), (10, 5000)]);
    fx.manual(current, 11, 5000);

    assert!(fx.detect_ids(batch).is_empty());
    assert_eq!(fx.count("SELECT COUNT(*) FROM transfer_group"), 0);
}

#[test]
fn different_currencies_never_pair() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let euros = fx.account_in(AccountKind::Current, Currency::EUR);
    let batch = fx.import(current, &[(10, -5000)]);
    fx.manual(euros, 10, 5000);

    assert!(fx.detect_ids(batch).is_empty());
}

#[test]
fn amounts_must_be_exactly_opposite_and_not_zero() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let batch = fx.import(current, &[(10, -5000), (10, 0), (10, 300)]);
    fx.manual(savings, 10, 4999);
    fx.manual(savings, 10, -5000);
    fx.manual(savings, 10, 0);
    fx.manual(savings, 10, 300);

    assert!(fx.detect_ids(batch).is_empty());
}

#[test]
fn window_includes_both_ends() {
    for (days, gap, pairs) in [
        (3, 3, true),
        (3, 4, false),
        (3, -3, true),
        (3, -4, false),
        (0, 0, true),
        (0, 1, false),
        (0, -1, false),
        (31, 31, true),
    ] {
        let mut fx = Fixture::new();
        let current = fx.account(AccountKind::Current);
        let savings = fx.account(AccountKind::Savings);
        let batch = fx.import(current, &[(15, -5000)]);
        fx.manual(savings, (15 + gap) as u32, 5000);

        let found = fx.detect(batch, window(days));
        assert_eq!(!found.is_empty(), pairs, "window {days}, gap {gap}");
    }
}

#[test]
fn rows_in_a_group_are_left_alone_whatever_its_status() {
    for status in TransferStatus::ALL {
        let mut fx = Fixture::new();
        let current = fx.account(AccountKind::Current);
        let savings = fx.account(AccountKind::Savings);
        let batch = fx.import(current, &[(10, -5000), (12, -800)]);
        let rows = fx.rows(batch);
        // A counterpart already grouped.
        let grouped = fx.manual(savings, 10, 5000);
        fx.put_in_group(grouped, status);
        // A batch row already grouped.
        fx.put_in_group(rows[1], status);
        fx.manual(savings, 12, 800);

        assert!(fx.detect_ids(batch).is_empty(), "{status:?}");
        assert_eq!(fx.count("SELECT COUNT(*) FROM transfer_group"), 2);
    }
}

#[test]
fn excluded_rows_are_left_alone() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let batch = fx.import(current, &[(10, -5000), (12, -800)]);
    let rows = fx.rows(batch);
    let excluded = fx.manual(savings, 10, 5000);
    fx.manual(savings, 12, 800);
    let exclude = "UPDATE \"transaction\" SET excluded_reason = 'duplicate', \
                   excluded_at = '2026-11-01T00:00:00.000Z' WHERE id = ?1";
    fx.db.conn().execute(exclude, [excluded]).unwrap();
    fx.db.conn().execute(exclude, [rows[1]]).unwrap();

    assert!(fx.detect_ids(batch).is_empty());
}

#[test]
fn identical_transfers_pair_one_to_one() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let out_batch = fx.import(current, &[(10, -5000), (10, -5000)]);
    let out = fx.rows(out_batch);
    let arrived = fx.import(savings, &[(10, 5000), (10, 5000)]);
    let arrived_rows = fx.rows(arrived);

    let pairs = fx.detect_ids(arrived);

    assert_eq!(
        pairs,
        vec![(out[0], arrived_rows[0]), (out[1], arrived_rows[1])]
    );
}

#[test]
fn three_out_two_in_leaves_one_unpaired() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let batch = fx.import(current, &[(10, -5000), (10, -5000), (10, -5000)]);
    let out = fx.rows(batch);
    let first = fx.manual(savings, 10, 5000);
    let second = fx.manual(savings, 10, 5000);

    assert_eq!(
        fx.detect_ids(batch),
        vec![(out[0], first), (out[1], second)]
    );
    assert_eq!(fx.group_id(out[2]), None);
}

#[test]
fn closest_date_wins() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let isa = fx.account(AccountKind::Savings);
    let batch = fx.import(current, &[(10, -5000)]);
    // Lower IDs, but further away.
    fx.manual(savings, 12, 5000);
    fx.manual(isa, 8, 5000);
    let closest = fx.manual(isa, 11, 5000);

    assert_eq!(fx.detect_ids(batch), vec![(fx.rows(batch)[0], closest)]);
}

#[test]
fn closest_pairs_are_taken_first_across_the_batch() {
    // Out on the 10th and the 12th; in on the 12th. The 12th's pair is
    // closer, so the 10th stays unpaired even though its row comes first.
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let batch = fx.import(current, &[(10, -5000), (12, -5000)]);
    let out = fx.rows(batch);
    let arrived = fx.manual(savings, 12, 5000);

    assert_eq!(fx.detect_ids(batch), vec![(out[1], arrived)]);
}

#[test]
fn on_the_same_day_the_closest_time_wins() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let mut line = StatementLine::new(day(10), money(-5000), "Out");
    line.time = Some(NaiveTime::from_hms_opt(10, 0, 0).unwrap());
    let batch = fx.import_lines(current, vec![line]);
    // Lower IDs: one without a time, one hours away.
    fx.manual(savings, 10, 5000);
    fx.manual_at(savings, 10, Some("18:00:00"), 5000);
    let closest = fx.manual_at(savings, 10, Some("10:02:00"), 5000);

    assert_eq!(fx.detect_ids(batch), vec![(fx.rows(batch)[0], closest)]);
}

#[test]
fn pairs_with_a_row_from_an_earlier_batch() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let earlier = fx.import(savings, &[(9, 12000)]);
    let later = fx.import(current, &[(10, -12000)]);

    assert_eq!(
        fx.detect_ids(later),
        vec![(fx.rows(later)[0], fx.rows(earlier)[0])]
    );
    // Detecting again finds nothing new.
    assert!(fx.detect_ids(later).is_empty());
    assert!(fx.detect_ids(earlier).is_empty());
    assert_eq!(fx.count("SELECT COUNT(*) FROM transfer_group"), 1);
}

#[test]
fn asset_and_liability_accounts_never_take_part() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let asset = fx.account(AccountKind::Asset);
    let liability = fx.account(AccountKind::Liability);
    let batch = fx.import(current, &[(10, -5000), (10, 7000)]);
    // The schema keeps transactions off these accounts; detection doesn't
    // rely on it.
    fx.db
        .conn()
        .execute_batch("DROP TRIGGER transaction_account_insert")
        .unwrap();
    fx.manual(asset, 10, 5000);
    fx.manual(liability, 10, -7000);

    assert!(fx.detect_ids(batch).is_empty());
}

#[test]
fn rows_after_their_account_closed_are_left_alone() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let batch = fx.import(current, &[(10, -5000), (12, -800)]);
    let out = fx.rows(batch);
    // The savings account closed on the 11th: a row on the closing day
    // pairs, one after it doesn't.
    let closing = fx.manual(savings, 11, 5000);
    fx.manual(savings, 12, 800);
    fx.db
        .conn()
        .execute(
            "UPDATE account SET closed_on = ?1 WHERE id = ?2",
            params![date_key(day(11)), savings],
        )
        .unwrap();

    assert_eq!(fx.detect_ids(batch), vec![(out[0], closing)]);
}

#[test]
fn results_do_not_depend_on_row_order() {
    // The same rows in two databases; one has SQLite return unordered rows
    // backwards, and the IDs are passed in a different order.
    let build = |reverse: bool| {
        let mut fx = Fixture::new();
        if reverse {
            fx.db
                .conn()
                .execute_batch("PRAGMA reverse_unordered_selects = ON")
                .unwrap();
        }
        let current = fx.account(AccountKind::Current);
        let savings = fx.account(AccountKind::Savings);
        let pot = fx.pot(current);
        fx.import(savings, &[(9, 5000), (10, 5000), (11, 5000), (10, -2000)]);
        fx.manual(pot, 10, 5000);
        fx.manual(pot, 10, 2000);
        let batch = fx.import(
            current,
            &[
                (10, -5000),
                (10, -5000),
                (12, -5000),
                (10, 2000),
                (10, -2000),
            ],
        );
        let mut ids = fx.rows(batch);
        if reverse {
            ids.reverse();
        }
        let tx = fx.db.conn_mut().transaction().unwrap();
        let pairs = detect(&tx, &ids, TransferWindow::DEFAULT).unwrap();
        tx.commit().unwrap();
        pairs
    };

    let forwards = build(false);
    assert_eq!(forwards.len(), 5);
    assert_eq!(forwards, build(true));
}

#[test]
fn runs_inside_the_callers_transaction() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let batch = fx.import(current, &[(10, -5000)]);
    fx.manual(savings, 10, 5000);

    let tx = fx.db.conn_mut().transaction().unwrap();
    assert_eq!(
        detect_for_batch(&tx, batch, TransferWindow::DEFAULT)
            .unwrap()
            .len(),
        1
    );
    drop(tx); // rolled back

    assert_eq!(fx.count("SELECT COUNT(*) FROM transfer_group"), 0);
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM \"transaction\" WHERE transfer_group_id IS NOT NULL"),
        0
    );
}

#[test]
fn unknown_ids_are_errors() {
    let mut fx = Fixture::new();
    assert!(matches!(
        suggest_for_batch(&mut fx.db, BatchId::new(99), TransferWindow::DEFAULT),
        Err(Error::BatchNotFound(_))
    ));
    assert!(matches!(
        detect(
            fx.db.conn(),
            &[TransactionId::new(99)],
            TransferWindow::DEFAULT
        ),
        Err(Error::TransactionNotFound(_))
    ));
    assert!(matches!(
        group(&fx.db, TransferGroupId::new(99)),
        Err(Error::GroupNotFound(_))
    ));
    assert!(matches!(
        group_of(&fx.db, TransactionId::new(99)),
        Err(Error::TransactionNotFound(_))
    ));
}

// Undo ----------------------------------------------------------------------

#[test]
fn undoing_one_side_dissolves_the_pair_and_reimporting_pairs_again() {
    let mut fx = Fixture::new();
    let current = fx.account(AccountKind::Current);
    let savings = fx.account(AccountKind::Savings);
    let arrived_batch = fx.import(savings, &[(10, 5000)]);
    let arrived = fx.rows(arrived_batch)[0];
    let out_batch = fx.import(current, &[(10, -5000)]);
    let pair = fx.detect(out_batch, TransferWindow::DEFAULT)[0];

    let undone = import::undo(&mut fx.db, out_batch).unwrap();

    assert_eq!(undone.transfers, 1);
    assert_eq!(fx.group_id(arrived), None);
    assert!(matches!(
        group(&fx.db, pair.group),
        Err(Error::GroupNotFound(_))
    ));
    // An undone batch has nothing left to pair.
    assert!(fx.detect_ids(out_batch).is_empty());

    let again = fx.import(current, &[(10, -5000)]);
    assert_eq!(fx.detect_ids(again), vec![(fx.rows(again)[0], arrived)]);
}
