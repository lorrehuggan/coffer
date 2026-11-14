//! Tests for the review queues' read model against a real database.

use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{EnteredBalance, EnteredSource, NewAccount};
use crate::category::CategoryKey;
use crate::duplicate::CandidateStatus;
use crate::exclusion::ExclusionReason;
use crate::import::{self, Committed};
use crate::manual::{self, ManualTransaction};
use crate::statement::{SourceFormat, SourceLocation, Statement, StatementLine};

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

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
            &NewAccount::new("Joint account", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        Fixture {
            db,
            account,
            _dir: dir,
        }
    }

    fn opening(&mut self, d: u32, minor: i64) {
        self.db
            .conn()
            .execute(
                "UPDATE account SET opening_balance_minor = ?1, opening_date = ?2 WHERE id = ?3",
                params![minor, date_key(day(d)), self.account],
            )
            .unwrap();
    }

    fn entered(&mut self, d: u32, minor: i64) {
        account::add_balance(
            &mut self.db,
            self.account,
            &EnteredBalance {
                date: day(d),
                balance: gbp(minor),
                source: EnteredSource::Manual,
                note: None,
            },
        )
        .unwrap();
    }

    /// Imports one row with a bank ID.
    fn import(&mut self, d: u32, minor: i64, text: &str, id: &str) -> TransactionId {
        let mut statement = Statement::new(SourceFormat::Ofx, Currency::GBP);
        let mut line = StatementLine::new(day(d), gbp(minor), text);
        line.external_id = Some(id.to_owned());
        statement.push(SourceLocation::Line(2), Ok(line));
        let preview = import::preview(&self.db, self.account, "joint.ofx", statement).unwrap();
        let Committed::Batch(summary) =
            import::commit(&mut self.db, &preview, preview.needs_confirmation()).unwrap()
        else {
            panic!("expected a batch");
        };
        self.db
            .conn()
            .query_row(
                "SELECT id FROM \"transaction\" WHERE batch_id = ?1",
                [summary.batch.get()],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn manual(&mut self, d: u32, minor: i64, text: &str, category: Option<&str>) -> TransactionId {
        let mut entry = ManualTransaction::new(self.account, day(d), gbp(minor), text);
        entry.category = category.map(|key| CategoryKey::new(key).unwrap());
        manual::create(&mut self.db, &entry).unwrap()
    }

    fn scan(&mut self) -> DuplicateCandidateId {
        let found = duplicate::scan(&mut self.db, None, DuplicateWindow::DEFAULT).unwrap();
        assert_eq!(found.len(), 1, "one pair");
        found[0].id
    }

    /// Opening £640.00 on the 1st; an imported −£42.50 Ocado on the 4th
    /// and the same typed in by hand on the 5th; £18.40 entered on the
    /// 9th, which matches only without the copy: it fails by £42.50.
    fn ocado() -> (Fixture, TransactionId, TransactionId, DuplicateCandidateId) {
        let mut fixture = Fixture::new();
        fixture.opening(1, 64_000);
        let imported = fixture.import(4, -4_250, "OCADO RETAIL LTD HATFIELD GB", "tx_0001");
        let typed = fixture.manual(5, -4_250, "OCADO.COM", Some("food.groceries"));
        fixture.manual(6, -57_910, "Rent share", None);
        fixture.entered(9, 1_840);
        let candidate = fixture.scan();
        (fixture, imported, typed, candidate)
    }
}

#[test]
fn a_pair_lists_the_kept_row_first_and_why_it_was_flagged() {
    let (fixture, imported, typed, candidate) = Fixture::ocado();
    let review = review(&fixture.db).unwrap();
    assert_eq!(review.pairs.len(), 1);
    let pair = &review.pairs[0];
    assert_eq!(pair.candidate, candidate);
    assert_eq!(pair.account, fixture.account);
    assert_eq!(pair.account_name, "Joint account");
    // The row with the bank's ID is kept by default.
    assert_eq!(pair.default_keep(), imported);
    assert_eq!(pair.rows[1].transaction, typed);
    assert_eq!(pair.other(imported).transaction, typed);

    let kept = &pair.rows[0];
    assert_eq!(kept.bank_id.as_deref(), Some("tx_0001"));
    assert_eq!(kept.bank_text, "OCADO RETAIL LTD HATFIELD GB");
    assert_eq!(kept.amount, gbp(-4_250));
    assert_eq!(kept.category, RowCategory::Uncategorised);
    assert!(matches!(&kept.added, Added::Imported { file, .. } if file == "joint.ofx"));
    let other = &pair.rows[1];
    assert!(other.is_manual());
    assert_eq!(other.bank_id, None);
    assert!(
        matches!(&other.category, RowCategory::Category(category) if category.key.as_str() == "food.groceries")
    );

    assert_eq!(
        pair.why,
        Why {
            days_apart: 1,
            window: DuplicateWindow::DEFAULT,
            same_text: false,
            bank_ids: BankIds::OneOnly,
            manual: true,
        }
    );
}

#[test]
fn the_effect_is_the_merge_rolled_back() {
    let (fixture, imported, typed, candidate) = Fixture::ocado();
    let effect = merge_effect(&fixture.db, MergeRequest::new(candidate)).unwrap();
    assert_eq!(effect.kept, imported);
    assert_eq!(effect.excluded, typed);
    // The typed row's category moves to the uncategorised kept row.
    assert_eq!(effect.carried, [Field::Category]);
    // Groceries stays the same (the category moves with the money); the
    // uncategorised spending loses the copy.
    assert_eq!(effect.spending.len(), 1);
    assert_eq!(effect.spending[0].category, None);
    assert_eq!(effect.spending[0].change, gbp(-4_250));
    assert_eq!(effect.locked, None);

    // The check on the 9th passes after it.
    assert_eq!(effect.checks.len(), 1);
    let change = effect.checks[0];
    assert_eq!(change.date, day(9));
    assert!(change.fixes());
    assert!(!change.breaks());
    assert_eq!(
        effect.count,
        Some(CountChange {
            date: day(9),
            before: gbp(-2_410),
            after: gbp(1_840),
        })
    );

    // Nothing changed.
    assert_eq!(
        duplicate::get(&fixture.db, candidate).unwrap().status,
        CandidateStatus::Pending
    );
    assert_eq!(crate::exclusion::get(&fixture.db, typed).unwrap(), None);
    let review = review(&fixture.db).unwrap();
    assert_eq!(review.failing(), 1);
}

#[test]
fn keeping_the_other_row_plans_the_other_way() {
    let (fixture, imported, typed, candidate) = Fixture::ocado();
    let pair = pair(&fixture.db, candidate, Some(typed)).unwrap();
    assert_eq!(pair.default_keep(), typed);
    let effect = pair.effect.unwrap();
    assert_eq!(effect.kept, typed);
    assert_eq!(effect.excluded, imported);
    // Nothing the user set is on the imported row, so nothing moves; the
    // uncategorised copy leaves spending.
    assert_eq!(effect.carried, []);
    assert_eq!(effect.spending.len(), 1);
    assert_eq!(effect.spending[0].change, gbp(-4_250));
}

#[test]
fn a_failing_check_names_its_base_window_and_the_pair_that_explains_it() {
    let (fixture, _, _, candidate) = Fixture::ocado();
    let review = review(&fixture.db).unwrap();
    assert_eq!(review.failing(), 1);
    assert_eq!(failing(&fixture.db).unwrap(), 1);
    let account = review.account(fixture.account).unwrap();
    assert_eq!(account.name, "Joint account");
    assert_eq!(account.failures(), 1);
    assert!(account.has_checks());
    assert_eq!(account.rows.len(), 2);

    let start = &account.rows[0];
    assert_eq!(start.check.outcome, Outcome::StartingPoint);
    assert_eq!(start.base, None);
    assert_eq!(start.days(), None);

    let failed = &account.rows[1];
    assert!(failed.check.outcome.is_failed());
    assert_eq!(failed.base, Some(gbp(64_000)));
    assert_eq!(failed.since(), Some(day(1)));
    assert_eq!(failed.days(), Some((day(2), day(9))));
    assert_eq!(failed.transactions, 3);
    assert_eq!(failed.explained_by, Some(candidate));
    // Nothing passes, so nothing can be locked.
    assert_eq!(account.lockable, []);
}

#[test]
fn a_locked_duplicate_is_planned_as_if_unlocked_and_reported() {
    let mut fixture = Fixture::new();
    fixture.opening(1, 10_000);
    let imported = fixture.import(3, -500, "CAFE NERO 0042", "tx_1");
    let typed = fixture.manual(4, -500, "Cafe Nero", None);
    // Counts both rows, so it passes now.
    fixture.entered(5, 9_000);
    let candidate = fixture.scan();
    reconciliation::lock(&mut fixture.db, fixture.account, day(5)).unwrap();

    let effect = merge_effect(&fixture.db, MergeRequest::new(candidate)).unwrap();
    assert_eq!(effect.kept, imported);
    assert_eq!(
        effect.locked,
        Some(LockedRow {
            candidate,
            transaction: typed,
            date: day(4),
            through: day(5),
        })
    );
    // The merge would break the check on the 5th.
    assert_eq!(effect.checks.len(), 1);
    assert!(effect.checks[0].breaks());
    // The lock is still there.
    assert_eq!(
        reconciliation::locked_through(&fixture.db, fixture.account).unwrap(),
        Some(day(5))
    );
    // And merging still refuses.
    let preview = duplicate::merge_preview(&fixture.db, &[MergeRequest::new(candidate)]).unwrap();
    assert!(matches!(
        duplicate::merge(&mut fixture.db, &preview),
        Err(duplicate::Error::Locked { .. })
    ));
}

#[test]
fn lockable_dates_are_passing_checks_after_the_lock_and_before_a_failure() {
    let mut fixture = Fixture::new();
    fixture.opening(1, 10_000);
    fixture.manual(2, -500, "Cash", None);
    fixture.entered(3, 9_500);
    fixture.entered(4, 9_500);
    fixture.entered(5, 9_500);
    fixture.entered(7, 9_000);
    fixture.entered(8, 9_500);

    let review = review(&fixture.db).unwrap();
    let account = review.account(fixture.account).unwrap();
    assert_eq!(account.lockable, [day(5), day(4), day(3)]);

    reconciliation::lock(&mut fixture.db, fixture.account, day(3)).unwrap();
    let review = super::review(&fixture.db).unwrap();
    let account = review.account(fixture.account).unwrap();
    assert_eq!(account.locked_through, Some(day(3)));
    assert_eq!(account.lockable, [day(5), day(4)]);
}

#[test]
fn accounts_with_nothing_to_check_are_named_and_failing_ones_come_first() {
    let mut fixture = Fixture::new();
    let passing = account::create(
        &mut fixture.db,
        &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
    )
    .unwrap();
    for (d, minor) in [(1, 5_000), (2, 5_000)] {
        account::add_balance(
            &mut fixture.db,
            passing,
            &EnteredBalance {
                date: day(d),
                balance: gbp(minor),
                source: EnteredSource::Statement,
                note: None,
            },
        )
        .unwrap();
    }
    account::create(
        &mut fixture.db,
        &NewAccount::new("Amex", AccountKind::CreditCard, Currency::GBP),
    )
    .unwrap();
    account::create(
        &mut fixture.db,
        &NewAccount::new("Stocks ISA", AccountKind::Asset, Currency::GBP),
    )
    .unwrap();
    let mut pot = NewAccount::new("Holiday", AccountKind::Pot, Currency::GBP);
    pot.parent = Some(fixture.account);
    account::create(&mut fixture.db, &pot).unwrap();
    // The joint account fails: 1.00 short on the 3rd.
    fixture.opening(1, 1_000);
    fixture.entered(3, 900);

    let review = review(&fixture.db).unwrap();
    let names: Vec<&str> = review.accounts.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Joint account", "Savings"]);
    assert_eq!(review.failing(), 1);
    assert_eq!(review.not_checked.no_balance, ["Amex"]);
    assert_eq!(review.not_checked.valued, ["Stocks ISA"]);
    assert_eq!(review.not_checked.pots, 1);
}

#[test]
fn an_excluded_row_makes_the_pair_unmergeable_and_says_why() {
    let (mut fixture, _, typed, candidate) = Fixture::ocado();
    crate::exclusion::exclude(&mut fixture.db, typed, ExclusionReason::Duplicate, None).unwrap();
    let review = review(&fixture.db).unwrap();
    let pair = review.pair(candidate).unwrap();
    let message = pair.effect.as_ref().unwrap_err();
    assert!(message.contains("excluded"), "{message}");
    assert!(!message.contains("OCADO"));
}
