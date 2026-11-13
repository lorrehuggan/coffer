//! Tests for applying every action kind: on import, and to past
//! transactions as a change batch.

use std::collections::BTreeSet;

use chrono::NaiveDate;
use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount, date_key};
use crate::category::CategoryKey;
use crate::change::{self, Field, Value};
use crate::db::Database;
use crate::exclusion::{self, ExclusionReason};
use crate::id::{AccountId, BatchId, RuleId, TransactionId};
use crate::import::{self, Committed};
use crate::manual::{self, ManualTransaction};
use crate::merchant::{self, DisplayName};
use crate::money::{Currency, Money};
use crate::note;
use crate::split;
use crate::statement::{SourceFormat, SourceLocation, Statement, StatementLine};
use crate::tag::{self, TagName};
use crate::transfer;

// Fixture ------------------------------------------------------------------------

struct Fixture {
    db: Database,
    account: AccountId,
    _dir: TempDir,
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn key(text: &str) -> CategoryKey {
    CategoryKey::new(text).unwrap()
}

fn share(bp: u64) -> Share {
    Share::new(bp).unwrap()
}

fn set_category(category: &str) -> RuleAction {
    RuleAction::SetCategory(key(category))
}

fn set_merchant(name: &str) -> RuleAction {
    RuleAction::SetMerchant(ActionText::new(name).unwrap())
}

fn set_note(text: &str) -> RuleAction {
    RuleAction::SetNote(ActionText::new(text).unwrap())
}

fn add_tags(names: &[&str]) -> RuleAction {
    RuleAction::AddTag(
        TagList::new(names.iter().map(|n| TagName::new(n).unwrap()).collect()).unwrap(),
    )
}

fn exclude(reason: ExcludeReason) -> RuleAction {
    RuleAction::exclude(reason).unwrap()
}

fn my_share(bp: u64) -> RuleAction {
    RuleAction::MyShare(share(bp))
}

fn split_template(lines: &[(&str, u64)]) -> RuleAction {
    RuleAction::SplitTemplate(
        SplitTemplate::new(
            lines
                .iter()
                .map(|(category, bp)| SplitLine::new(key(category), share(*bp)))
                .collect(),
        )
        .unwrap(),
    )
}

/// A rule matching merchants containing `text`.
fn rule_on(text: &str, actions: Vec<RuleAction>) -> RuleDefinition {
    RuleDefinition::new(
        MatchField::Merchant,
        Pattern::new(MatchKind::Contains, text).unwrap(),
        AmountBounds::ANY,
        None,
        actions,
    )
    .unwrap()
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

    /// Stores a rule below every existing one.
    fn rule(&mut self, text: &str, actions: Vec<RuleAction>) -> RuleId {
        let last = list(&self.db).unwrap().len();
        create_at(&mut self.db, &rule_on(text, actions), last).unwrap()
    }

    /// Imports one row per `(day, amount, merchant)` into the account and
    /// returns their IDs in order.
    fn import(&mut self, rows: &[(u32, i64, &str)]) -> Vec<TransactionId> {
        let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
        for (index, (d, minor, merchant)) in rows.iter().enumerate() {
            let mut line = StatementLine::new(day(*d), gbp(*minor), format!("CARD {merchant}"));
            line.merchant = Some((*merchant).to_owned());
            statement.push(SourceLocation::Line(index as u64 + 2), Ok(line));
        }
        let preview = import::preview(&self.db, self.account, "s.csv", statement).unwrap();
        let Committed::Batch(summary) =
            import::commit(&mut self.db, &preview, preview.needs_confirmation()).unwrap()
        else {
            panic!("expected a batch");
        };
        self.batch_rows(summary.batch)
    }

    fn batch_rows(&self, batch: BatchId) -> Vec<TransactionId> {
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

    fn sql(&self, sql: &str, id: TransactionId) {
        self.db.conn().execute(sql, [id]).unwrap();
    }

    fn lock(&self, d: u32) {
        self.db
            .conn()
            .execute(
                "UPDATE account SET reconciled_through = ?1 WHERE id = ?2",
                params![date_key(day(d)), self.account],
            )
            .unwrap();
    }

    fn unlock(&self) {
        self.db
            .conn()
            .execute(
                "UPDATE account SET reconciled_through = NULL WHERE id = ?1",
                [self.account],
            )
            .unwrap();
    }

    /// Category key, category source and rule.
    fn category(&self, id: TransactionId) -> (Option<String>, Option<String>, Option<RuleId>) {
        self.db
            .conn()
            .query_row(
                "SELECT c.key, t.category_source, t.rule_id FROM \"transaction\" t \
                 LEFT JOIN category c ON c.id = t.category_id WHERE t.id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
    }

    /// Display merchant and its source.
    fn merchant(&self, id: TransactionId) -> (Option<String>, Option<String>) {
        self.db
            .conn()
            .query_row(
                "SELECT merchant_display, merchant_display_source FROM \"transaction\" \
                 WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    }

    fn note(&self, id: TransactionId) -> Option<String> {
        note::get(&self.db, id).unwrap()
    }

    fn tags(&self, id: TransactionId) -> Vec<String> {
        tag::tags_of(&self.db, id)
            .unwrap()
            .into_iter()
            .map(|tag| tag.name.as_str().to_owned())
            .collect()
    }

    fn excluded(&self, id: TransactionId) -> Option<ExclusionReason> {
        exclusion::get(&self.db, id)
            .unwrap()
            .map(|exclusion| exclusion.reason)
    }

    /// `my_share_minor` and `share_label`.
    fn share(&self, id: TransactionId) -> (Option<i64>, Option<String>) {
        self.db
            .conn()
            .query_row(
                "SELECT my_share_minor, share_label FROM \"transaction\" WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    }

    /// Split lines: category key, amount, share.
    fn lines(&self, id: TransactionId) -> Vec<(String, i64, Option<i64>)> {
        split::lines(&self.db, id)
            .unwrap()
            .into_iter()
            .map(|line| {
                (
                    line.category
                        .map(|key| key.as_str().to_owned())
                        .unwrap_or_default(),
                    line.amount.minor(),
                    line.my_share.map(|share| share.minor()),
                )
            })
            .collect()
    }

    /// Every field a change batch can hold, as it is now.
    fn snapshot(&self, id: TransactionId) -> Vec<Value> {
        Field::ALL
            .iter()
            .map(|field| change::read(self.db.conn(), id, *field).unwrap())
            .collect()
    }

    fn preview(&self, rule: RuleId) -> ApplyPreview {
        preview_apply(&self.db, rule).unwrap()
    }

    fn commit(&mut self, preview: &ApplyPreview, included: &[TransactionId]) -> Applied {
        commit_apply(&mut self.db, preview, &included.iter().copied().collect()).unwrap()
    }
}

fn s(text: &str) -> Option<String> {
    Some(text.to_owned())
}

fn ids<'a>(rows: impl Iterator<Item = &'a RowPlan>) -> Vec<TransactionId> {
    rows.map(|row| row.transaction).collect()
}

// Evaluating --------------------------------------------------------------------

fn rule_set(rules: Vec<RuleDefinition>) -> RuleSet {
    RuleSet::new(
        rules
            .into_iter()
            .enumerate()
            .map(|(i, rule)| StoredRule::new(RuleId::new(i as i64 + 1), i as i64, rule)),
    )
}

fn input(merchant: &str) -> MatchInput<'_> {
    MatchInput {
        account_id: AccountId::new(1),
        amount_minor: -1_000,
        merchant: Some(merchant),
        description: "CARD PAYMENT",
        source_category: None,
        source_type: None,
    }
}

#[test]
fn rules_win_field_by_field_and_tags_add_up() {
    let rules = rule_set(vec![
        rule_on("acme", vec![set_category("shopping"), set_note("first")]),
        rule_on(
            "acme",
            vec![
                set_category("food"),
                set_merchant("Acme"),
                set_note("second"),
                add_tags(&["Work", "travel"]),
            ],
        ),
        rule_on("acme", vec![add_tags(&["work", "gift"]), my_share(2_500)]),
        rule_on("tesco", vec![exclude(ExcludeReason::Disputed)]),
    ]);
    let outcome = rules.evaluate(&input("ACME LTD"));
    let by = |rule: i64| RuleId::new(rule);
    assert_eq!(outcome.category.as_ref().unwrap().rule, by(1));
    assert_eq!(outcome.category.unwrap().value, key("shopping"));
    assert_eq!(outcome.note.unwrap().rule, by(1));
    assert_eq!(outcome.merchant.unwrap().rule, by(2));
    assert_eq!(
        outcome.share.unwrap(),
        SetBy {
            value: share(2_500),
            rule: by(3)
        }
    );
    assert_eq!(outcome.exclude, None);
    assert_eq!(outcome.split, None);
    let tags: Vec<(&str, i64)> = outcome
        .tags
        .iter()
        .map(|set| (set.value.as_str(), set.rule.get()))
        .collect();
    // `work` repeats `Work` ignoring case: kept once, credited to rule 2.
    assert_eq!(tags, [("Work", 2), ("travel", 2), ("gift", 3)]);
}

#[test]
fn category_and_split_are_one_field() {
    let split = || split_template(&[("food", 5_000), ("home", 5_000)]);
    let rules = rule_set(vec![
        rule_on("acme", vec![split()]),
        rule_on("acme", vec![set_category("shopping")]),
    ]);
    let outcome = rules.evaluate(&input("ACME"));
    assert_eq!(outcome.category, None);
    assert_eq!(outcome.split.unwrap().rule, RuleId::new(1));

    let rules = rule_set(vec![
        rule_on("acme", vec![set_category("shopping")]),
        rule_on("acme", vec![split()]),
    ]);
    let outcome = rules.evaluate(&input("ACME"));
    assert_eq!(outcome.category.unwrap().rule, RuleId::new(1));
    assert_eq!(outcome.split, None);
}

#[test]
fn evaluating_one_rule_gives_the_fields_it_wins() {
    let rules = rule_set(vec![
        rule_on("acme", vec![set_category("shopping"), add_tags(&["work"])]),
        rule_on(
            "acme",
            vec![
                set_category("food"),
                set_note("n"),
                add_tags(&["Work", "x"]),
            ],
        ),
    ]);
    let second = rules.evaluate_rule(RuleId::new(2), &input("ACME")).unwrap();
    assert_eq!(second.category, None);
    assert_eq!(second.note.unwrap().rule, RuleId::new(2));
    // Every tag it names, even one a rule above adds.
    let tags: Vec<&str> = second.tags.iter().map(|set| set.value.as_str()).collect();
    assert_eq!(tags, ["Work", "x"]);
    assert_eq!(rules.evaluate_rule(RuleId::new(2), &input("TESCO")), None);
    assert_eq!(rules.evaluate_rule(RuleId::new(9), &input("ACME")), None);
    assert!(
        rules
            .evaluate_rule(RuleId::new(1), &input("ACME"))
            .is_some()
    );
}

// Import -------------------------------------------------------------------------

#[test]
fn every_action_applies_at_import() {
    let mut f = Fixture::new();
    let rule = f.rule(
        "acme",
        vec![
            set_category("shopping"),
            set_merchant("  Acme Ltd "),
            add_tags(&["work", "Travel"]),
            set_note(" Claim back "),
            exclude(ExcludeReason::Disputed),
            my_share(5_000),
        ],
    );
    let rows = f.import(&[(3, -1_001, "ACME 1234"), (4, -500, "TESCO")]);
    let (acme, tesco) = (rows[0], rows[1]);

    assert_eq!(f.category(acme), (s("shopping"), s("rule"), Some(rule)));
    assert_eq!(f.merchant(acme), (s("Acme Ltd"), s("rule")));
    assert_eq!(f.tags(acme), ["Travel", "work"]);
    assert_eq!(f.note(acme), s("Claim back"));
    assert_eq!(f.excluded(acme), Some(ExclusionReason::Disputed));
    // -500.5 rounds away from zero.
    assert_eq!(f.share(acme), (Some(-501), None));

    assert_eq!(f.category(tesco), (None, None, None));
    assert_eq!(f.merchant(tesco).1, None);
    assert!(f.tags(tesco).is_empty());
    assert_eq!(f.note(tesco), None);
    assert_eq!(f.excluded(tesco), None);
    assert_eq!(f.share(tesco), (None, None));

    // An import records no change batch.
    assert!(change::list(&f.db).unwrap().is_empty());
}

#[test]
fn a_whole_share_is_stored_as_all_of_it() {
    let mut f = Fixture::new();
    f.rule("acme", vec![my_share(10_000)]);
    f.rule("tesco", vec![my_share(0)]);
    let rows = f.import(&[(3, -999, "ACME"), (3, 999, "TESCO")]);
    assert_eq!(f.share(rows[0]), (None, None));
    assert_eq!(f.share(rows[1]), (Some(0), None));
}

#[test]
fn a_split_template_splits_exactly_at_import_with_the_share_on_each_line() {
    let mut f = Fixture::new();
    let rule = f.rule(
        "acme",
        vec![
            split_template(&[("food.groceries", 6_667), ("home", 3_333)]),
            my_share(5_000),
        ],
    );
    f.rule("acme", vec![set_category("shopping")]);
    let rows = f.import(&[(3, -1_001, "ACME"), (4, 300, "ACME REFUND")]);

    // 667.3667 and 333.6333: the left-over unit goes to the larger
    // remainder. Shares of -667 and -334 round half away from zero.
    assert_eq!(
        f.lines(rows[0]),
        [
            ("food.groceries".to_owned(), -667, Some(-334)),
            ("home".to_owned(), -334, Some(-167)),
        ]
    );
    assert_eq!(f.category(rows[0]), (None, None, None));
    assert_eq!(f.share(rows[0]), (None, None));
    // A refund splits with its own sign.
    assert_eq!(
        f.lines(rows[1]),
        [
            ("food.groceries".to_owned(), 200, Some(100)),
            ("home".to_owned(), 100, Some(50)),
        ]
    );
    let _ = rule;
}

#[test]
fn a_split_too_small_or_in_a_transfer_is_skipped_without_failing_the_import() {
    let mut f = Fixture::new();
    f.rule(
        "acme",
        vec![
            split_template(&[("food", 3_334), ("home", 3_333), ("bills", 3_333)]),
            my_share(5_000),
        ],
    );
    // Three lines need three minor units; two can't split, three can.
    let rows = f.import(&[(3, -2, "ACME"), (3, -3, "ACME"), (3, 0, "ACME")]);
    assert!(f.lines(rows[0]).is_empty());
    assert_eq!(f.category(rows[0]), (None, None, None));
    // Not split, so the share applies to the row.
    assert_eq!(f.share(rows[0]), (Some(-1), None));
    assert_eq!(f.lines(rows[1]).len(), 3);
    assert!(f.lines(rows[2]).is_empty());
    assert_eq!(f.share(rows[2]), (Some(0), None));

    // A row in a confirmed transfer (a pot's mirrored row, say) keeps its
    // category.
    let savings = account::create(
        &mut f.db,
        &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
    )
    .unwrap();
    let conn = f.db.conn();
    let insert = |account: AccountId, minor: i64| {
        conn.execute(
            "INSERT INTO import_batch (account_id, source_file, format) \
             VALUES (?1, 's.csv', 'csv')",
            [account],
        )
        .unwrap();
        let batch = BatchId::new(conn.last_insert_rowid());
        conn.execute(
            "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
             merchant, description_raw, description_normalized, fingerprint, origin, \
             category_id, category_source) \
             VALUES (?1, ?2, '2026-11-05', ?3, 'GBP', 'ACME', 'ACME', 'ACME', 'fp', 'imported', \
             (SELECT id FROM category WHERE key = 'transfer'), 'preset_map')",
            params![account, batch, minor],
        )
        .unwrap();
        (batch, TransactionId::new(conn.last_insert_rowid()))
    };
    let (batch, out) = insert(f.account, -1_000);
    let (_, into) = insert(savings, 1_000);
    transfer::record_confirmed_pair(conn, out, into).unwrap();
    let applied = apply_to_batch(conn, batch).unwrap();
    assert_eq!(applied.splits_skipped, 1);
    assert_eq!(applied.split, 0);
    assert_eq!(f.category(out).0, s("transfer"));
    assert!(f.lines(out).is_empty());
    assert_eq!(f.share(out), (Some(-500), None));
}

#[test]
fn rules_combine_field_by_field_at_import() {
    let mut f = Fixture::new();
    let first = f.rule("acme", vec![set_category("shopping"), add_tags(&["Work"])]);
    let second = f.rule(
        "acme",
        vec![
            set_category("food"),
            set_merchant("Acme"),
            add_tags(&["work", "gift"]),
        ],
    );
    let rows = f.import(&[(3, -1_000, "ACME")]);
    assert_eq!(f.category(rows[0]), (s("shopping"), s("rule"), Some(first)));
    assert_eq!(f.merchant(rows[0]), (s("Acme"), s("rule")));
    assert_eq!(f.tags(rows[0]), ["gift", "Work"]);
    let _ = second;
    assert_eq!(tag::list(&f.db).unwrap().len(), 2);
}

#[test]
fn tags_are_found_ignoring_case_or_created() {
    let mut f = Fixture::new();
    let work = tag::create(&mut f.db, &TagName::new("WORK").unwrap(), true).unwrap();
    f.rule("acme", vec![add_tags(&["work", "new tag"])]);
    let rows = f.import(&[(3, -1_000, "ACME"), (4, -1_000, "ACME")]);
    assert_eq!(f.tags(rows[0]), ["new tag", "WORK"]);
    assert_eq!(f.tags(rows[1]), ["new tag", "WORK"]);
    let tags = tag::list(&f.db).unwrap();
    assert_eq!(tags.len(), 2);
    // The existing tag stays private; a created one isn't.
    assert!(tags.iter().any(|tag| tag.id == work && tag.private));
    assert!(tags.iter().any(|tag| tag.id != work && !tag.private));
}

#[test]
fn import_never_changes_a_value_the_user_set() {
    let mut f = Fixture::new();
    f.rule(
        "acme",
        vec![
            set_category("shopping"),
            set_merchant("Acme"),
            set_note("rule note"),
            exclude(ExcludeReason::Disputed),
            my_share(5_000),
            add_tags(&["work"]),
        ],
    );
    // Rows already holding user values, in a batch the rules then run on.
    let conn = f.db.conn();
    conn.execute(
        "INSERT INTO import_batch (account_id, source_file, format) VALUES (?1, 's.csv', 'csv')",
        [f.account],
    )
    .unwrap();
    let batch = BatchId::new(conn.last_insert_rowid());
    conn.execute(
        "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
         merchant, description_raw, description_normalized, fingerprint, origin, \
         category_id, category_source, merchant_display, merchant_display_source, user_note, \
         excluded_reason, excluded_at, my_share_minor) \
         VALUES (?1, ?2, '2026-11-05', -1000, 'GBP', 'ACME', 'ACME', 'ACME', 'fp', 'imported', \
         (SELECT id FROM category WHERE key = 'bills'), 'user', 'Mine', 'user', 'my note', \
         'bank_error', '2026-11-05T10:00:00.000Z', -100)",
        params![f.account, batch],
    )
    .unwrap();
    let row = TransactionId::new(conn.last_insert_rowid());
    let applied = apply_to_batch(conn, batch).unwrap();
    assert_eq!(applied.categorized, 0);
    assert_eq!(f.category(row), (s("bills"), s("user"), None));
    assert_eq!(f.merchant(row), (s("Mine"), s("user")));
    assert_eq!(f.note(row), s("my note"));
    assert_eq!(f.excluded(row), Some(ExclusionReason::BankError));
    assert_eq!(f.share(row), (Some(-100), None));
    // Adding a tag overwrites nothing.
    assert_eq!(f.tags(row), ["work"]);
}

#[test]
fn a_rule_note_too_long_fails_naming_the_rule() {
    let mut f = Fixture::new();
    let rule = f.rule("acme", vec![set_note(&"n".repeat(2_001))]);
    let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
    let mut line = StatementLine::new(day(3), gbp(-1), "CARD");
    line.merchant = Some("ACME".into());
    statement.push(SourceLocation::Line(2), Ok(line));
    let preview = import::preview(&f.db, f.account, "s.csv", statement).unwrap();
    let err = import::commit(&mut f.db, &preview, preview.needs_confirmation()).unwrap_err();
    assert!(
        matches!(
            &err,
            import::Error::Rules(StoreError::Invalid { id, problem: InvalidRule::Action(ActionKind::SetNote) })
                if *id == rule
        ),
        "{err:?}"
    );
    assert!(!err.to_string().contains("nnn"));
    let count: i64 =
        f.db.conn()
            .query_row("SELECT COUNT(*) FROM \"transaction\"", [], |row| row.get(0))
            .unwrap();
    assert_eq!(count, 0, "the import rolled back");
}

// Apply to history ---------------------------------------------------------------

#[test]
fn apply_sets_every_field_and_undo_restores_exactly() {
    let mut f = Fixture::new();
    let rows = f.import(&[(3, -1_001, "ACME"), (4, -500, "TESCO"), (5, -77, "ACME")]);
    let before: Vec<Vec<Value>> = rows.iter().map(|id| f.snapshot(*id)).collect();
    let rule = f.rule(
        "acme",
        vec![
            set_category("shopping"),
            set_merchant("Acme"),
            add_tags(&["work"]),
            set_note("claim"),
            exclude(ExcludeReason::BankError),
            my_share(5_000),
        ],
    );

    let preview = f.preview(rule);
    assert_eq!(ids(preview.automatic_rows()), [rows[0], rows[2]]);
    assert_eq!(ids(preview.user_set_rows()), []);
    assert_eq!(ids(preview.locked_rows()), []);
    let fields: Vec<Field> = preview.rows[0]
        .changes
        .iter()
        .map(|change| change.new.field())
        .collect();
    assert_eq!(
        fields,
        [
            Field::Merchant,
            Field::UserNote,
            Field::Tags,
            Field::Exclusion,
            Field::Category,
            Field::Share
        ]
    );
    // The preview wrote nothing (not even the tag).
    assert_eq!(f.snapshot(rows[0]), before[0]);
    assert!(tag::list(&f.db).unwrap().is_empty());

    let applied = f.commit(&preview, &[]);
    assert_eq!((applied.transactions, applied.items), (2, 12));
    let batch = applied.batch.unwrap();
    assert_eq!(
        change::get(&f.db, batch).unwrap().kind,
        change::ChangeKind::RuleApply
    );
    for id in [rows[0], rows[2]] {
        assert_eq!(f.category(id), (s("shopping"), s("rule"), Some(rule)));
        assert_eq!(f.merchant(id), (s("Acme"), s("rule")));
        assert_eq!(f.tags(id), ["work"]);
        assert_eq!(f.note(id), s("claim"));
        assert_eq!(f.excluded(id), Some(ExclusionReason::BankError));
    }
    assert_eq!(f.share(rows[0]), (Some(-501), None));
    assert_eq!(f.share(rows[2]), (Some(-39), None));
    assert_eq!(f.snapshot(rows[1]), before[1]);

    // Applying again changes nothing.
    assert!(f.preview(rule).is_empty());

    let undone = change::undo(&mut f.db, batch).unwrap();
    assert_eq!((undone.restored, undone.changed_since), (12, 0));
    for (id, before) in rows.iter().zip(&before) {
        assert_eq!(&f.snapshot(*id), before);
    }
    // The tag the apply created stays, unlinked.
    assert_eq!(tag::list(&f.db).unwrap().len(), 1);
}

#[test]
fn undo_skips_rows_changed_since() {
    let mut f = Fixture::new();
    let rows = f.import(&[(3, -1_000, "ACME"), (4, -1_000, "ACME")]);
    let rule = f.rule("acme", vec![set_note("claim"), set_category("shopping")]);
    let preview = f.preview(rule);
    let batch = f.commit(&preview, &[]).batch.unwrap();
    note::set(&mut f.db, rows[0], "mine now").unwrap();
    let undone = change::undo(&mut f.db, batch).unwrap();
    assert_eq!((undone.restored, undone.changed_since), (3, 1));
    assert_eq!(f.note(rows[0]), s("mine now"));
    assert_eq!(f.note(rows[1]), None);
    assert_eq!(f.category(rows[0]), (None, None, None));
}

#[test]
fn apply_changes_rule_and_preset_values_and_lists_user_values_separately() {
    let mut f = Fixture::new();
    let rows = f.import(&[
        (1, -1_000, "ACME"), // uncategorised
        (2, -1_000, "ACME"), // preset category
        (3, -1_000, "ACME"), // another rule's category
        (4, -1_000, "ACME"), // the user's category
        (5, -1_000, "ACME"), // approved suggestion
        (6, -1_000, "ACME"), // the user's name, note, exclusion and share
        (7, -1_000, "ACME"), // the user's category, the same one
    ]);
    let other = f.rule("nothing", vec![set_category("bills")]);
    f.sql(
        "UPDATE \"transaction\" SET category_id = (SELECT id FROM category WHERE key = 'food'), \
         category_source = 'preset_map' WHERE id = ?1",
        rows[1],
    );
    f.db.conn()
        .execute(
            "UPDATE \"transaction\" SET category_id = (SELECT id FROM category WHERE key = 'food'), \
             category_source = 'rule', rule_id = ?1 WHERE id = ?2",
            params![other, rows[2]],
        )
        .unwrap();
    f.sql(
        "UPDATE \"transaction\" SET category_id = (SELECT id FROM category WHERE key = 'food'), \
         category_source = 'user' WHERE id = ?1",
        rows[3],
    );
    f.sql(
        "UPDATE \"transaction\" SET category_id = (SELECT id FROM category WHERE key = 'food'), \
         category_source = 'ai_approved' WHERE id = ?1",
        rows[4],
    );
    merchant::set_user_name(&mut f.db, rows[5], &DisplayName::new("Mine").unwrap()).unwrap();
    note::set(&mut f.db, rows[5], "my note").unwrap();
    exclusion::exclude(&mut f.db, rows[5], ExclusionReason::Duplicate, None).unwrap();
    f.sql(
        "UPDATE \"transaction\" SET my_share_minor = -100, share_label = 'Sam' WHERE id = ?1",
        rows[5],
    );
    f.sql(
        "UPDATE \"transaction\" SET category_id = (SELECT id FROM category WHERE key = 'shopping'), \
         category_source = 'user' WHERE id = ?1",
        rows[6],
    );

    let rule = f.rule(
        "acme",
        vec![
            set_category("shopping"),
            set_merchant("Acme"),
            set_note("rule note"),
            exclude(ExcludeReason::Disputed),
            my_share(5_000),
        ],
    );
    let preview = f.preview(rule);
    assert_eq!(ids(preview.user_set_rows()), [rows[3], rows[4], rows[5]]);
    // Every row has something to change without inclusion: the name, note,
    // exclusion and share on the others; the category on rows[5].
    assert_eq!(ids(preview.automatic_rows()), rows);
    let user_fields: Vec<Field> = preview.rows[5]
        .changes
        .iter()
        .filter(|change| change.user_set)
        .map(|change| change.new.field())
        .collect();
    assert_eq!(
        user_fields,
        [
            Field::Merchant,
            Field::UserNote,
            Field::Exclusion,
            Field::Share
        ]
    );
    // The same category the user chose isn't a change.
    assert!(
        preview.rows[6]
            .changes
            .iter()
            .all(|change| change.new.field() != Field::Category)
    );

    // Nothing included: the user's values stay.
    f.commit(&preview, &[]);
    for id in &rows[..3] {
        assert_eq!(f.category(*id), (s("shopping"), s("rule"), Some(rule)));
    }
    assert_eq!(f.category(rows[3]), (s("food"), s("user"), None));
    assert_eq!(f.category(rows[4]), (s("food"), s("ai_approved"), None));
    assert_eq!(f.category(rows[6]), (s("shopping"), s("user"), None));
    assert_eq!(f.merchant(rows[5]), (s("Mine"), s("user")));
    assert_eq!(f.note(rows[5]), s("my note"));
    assert_eq!(f.excluded(rows[5]), Some(ExclusionReason::Duplicate));
    assert_eq!(f.share(rows[5]), (Some(-100), s("Sam")));
    assert_eq!(f.category(rows[5]), (s("shopping"), s("rule"), Some(rule)));
    // Their other fields changed.
    assert_eq!(f.merchant(rows[3]), (s("Acme"), s("rule")));
    assert_eq!(f.note(rows[4]), s("rule note"));

    // Including some of them changes their values, and only theirs.
    let preview = f.preview(rule);
    assert_eq!(ids(preview.user_set_rows()), [rows[3], rows[4], rows[5]]);
    assert_eq!(ids(preview.automatic_rows()), []);
    f.commit(&preview, &[rows[3], rows[5]]);
    assert_eq!(f.category(rows[3]), (s("shopping"), s("rule"), Some(rule)));
    assert_eq!(f.category(rows[4]), (s("food"), s("ai_approved"), None));
    assert_eq!(f.merchant(rows[5]), (s("Acme"), s("rule")));
    assert_eq!(f.note(rows[5]), s("rule note"));
    assert_eq!(f.excluded(rows[5]), Some(ExclusionReason::Disputed));
    // The label stays with the new share.
    assert_eq!(f.share(rows[5]), (Some(-500), s("Sam")));
}

#[test]
fn only_rows_with_user_values_can_be_included() {
    let mut f = Fixture::new();
    let rows = f.import(&[(1, -1_000, "ACME"), (2, -1_000, "TESCO")]);
    let rule = f.rule("acme", vec![set_category("shopping")]);
    let preview = f.preview(rule);
    for id in [rows[0], rows[1], TransactionId::new(999)] {
        let err = commit_apply(&mut f.db, &preview, &BTreeSet::from([id])).unwrap_err();
        assert!(
            matches!(err, ApplyError::NotIncludable(at) if at == id),
            "{err:?}"
        );
    }
    assert_eq!(f.category(rows[0]), (None, None, None));
}

#[test]
fn a_locked_exclusion_is_flagged_and_refused_until_unlocked() {
    let mut f = Fixture::new();
    let rows = f.import(&[(3, -1_000, "ACME"), (10, -1_000, "ACME")]);
    let rule = f.rule(
        "acme",
        vec![exclude(ExcludeReason::Disputed), set_category("shopping")],
    );
    f.lock(5);
    let preview = f.preview(rule);
    assert_eq!(ids(preview.locked_rows()), [rows[0]]);
    let locked: Vec<_> = preview.rows[0]
        .changes
        .iter()
        .filter_map(|change| change.locked.map(|through| (change.new.field(), through)))
        .collect();
    // Only the exclusion meets the lock, whatever its reason.
    assert_eq!(locked, [(Field::Exclusion, day(5))]);
    let err = commit_apply(&mut f.db, &preview, &BTreeSet::new()).unwrap_err();
    assert!(
        matches!(&err, ApplyError::Locked(ids) if *ids == [rows[0]]),
        "{err:?}"
    );
    assert_eq!(f.excluded(rows[1]), None, "nothing was written");

    f.unlock();
    let preview = f.preview(rule);
    assert_eq!(ids(preview.locked_rows()), []);
    let batch = f.commit(&preview, &[]).batch.unwrap();
    assert_eq!(f.excluded(rows[0]), Some(ExclusionReason::Disputed));
    assert_eq!(f.excluded(rows[1]), Some(ExclusionReason::Disputed));

    // Undoing it under a lock is refused too.
    f.lock(5);
    assert!(matches!(
        change::undo(&mut f.db, batch),
        Err(change::Error::Locked { .. })
    ));
}

#[test]
fn a_locked_user_exclusion_left_out_doesnt_block_the_apply() {
    let mut f = Fixture::new();
    let rows = f.import(&[(3, -1_000, "ACME")]);
    exclusion::exclude(&mut f.db, rows[0], ExclusionReason::BankError, None).unwrap();
    let rule = f.rule(
        "acme",
        vec![exclude(ExcludeReason::Disputed), set_category("shopping")],
    );
    f.lock(5);
    let preview = f.preview(rule);
    assert_eq!(ids(preview.locked_rows()), [rows[0]]);
    assert_eq!(ids(preview.user_set_rows()), [rows[0]]);
    // Not included: the locked change isn't made, so the apply goes ahead.
    f.commit(&preview, &[]);
    assert_eq!(f.category(rows[0]).0, s("shopping"));
    assert_eq!(f.excluded(rows[0]), Some(ExclusionReason::BankError));
    // Included: refused.
    let preview = f.preview(rule);
    assert_eq!(ids(preview.locked_rows()), [rows[0]]);
    let err = commit_apply(&mut f.db, &preview, &BTreeSet::from([rows[0]])).unwrap_err();
    assert!(matches!(err, ApplyError::Locked(_)), "{err:?}");
}

#[test]
fn other_fields_under_a_lock_arent_flagged() {
    let mut f = Fixture::new();
    let rows = f.import(&[(3, -1_000, "ACME")]);
    let rule = f.rule(
        "acme",
        vec![set_category("shopping"), set_note("n"), my_share(5_000)],
    );
    f.lock(5);
    let preview = f.preview(rule);
    assert_eq!(ids(preview.locked_rows()), []);
    f.commit(&preview, &[]);
    assert_eq!(f.note(rows[0]), s("n"));
}

#[test]
fn a_stale_preview_is_refused() {
    let mut f = Fixture::new();
    let rows = f.import(&[(3, -1_000, "ACME"), (4, -1_000, "ACME")]);
    let rule = f.rule("acme", vec![set_note("claim")]);
    let preview = f.preview(rule);
    note::set(&mut f.db, rows[1], "mine").unwrap();
    assert!(matches!(
        commit_apply(&mut f.db, &preview, &BTreeSet::new()),
        Err(ApplyError::Stale)
    ));
    assert_eq!(f.note(rows[0]), None, "nothing was written");

    // A rule edited since is stale too.
    let preview = f.preview(rule);
    f.db.conn()
        .execute(
            "UPDATE rule_action SET value_json = '\"other\"' WHERE rule_id = ?1",
            [rule],
        )
        .unwrap();
    assert!(matches!(
        commit_apply(&mut f.db, &preview, &BTreeSet::new()),
        Err(ApplyError::Stale)
    ));

    // A new matching row too (a manual one: an import would run the rule).
    let preview = f.preview(rule);
    let mut entry = ManualTransaction::new(f.account, day(5), gbp(-1_000), "Card");
    entry.merchant = Some("ACME".into());
    manual::create(&mut f.db, &entry).unwrap();
    assert!(matches!(
        commit_apply(&mut f.db, &preview, &BTreeSet::new()),
        Err(ApplyError::Stale)
    ));
}

#[test]
fn apply_changes_only_the_fields_the_rule_wins() {
    let mut f = Fixture::new();
    let rows = f.import(&[(3, -1_000, "ACME")]);
    let top = f.rule("acme", vec![set_category("food")]);
    let rule = f.rule("acme", vec![set_category("shopping"), set_note("n")]);
    let preview = f.preview(rule);
    f.commit(&preview, &[]);
    // The category is the top rule's to set: applying the lower one leaves
    // it.
    assert_eq!(f.category(rows[0]), (None, None, None));
    assert_eq!(f.note(rows[0]), s("n"));
    let _ = top;
    assert!(matches!(
        preview_apply(&f.db, RuleId::new(999)),
        Err(ApplyError::Rules(StoreError::NotFound(_)))
    ));
}

#[test]
fn apply_skips_split_rows_and_splits_others_undoably() {
    let mut f = Fixture::new();
    let rows = f.import(&[(3, -1_000, "ACME"), (4, -1_000, "ACME"), (5, -1, "ACME")]);
    split::set(
        &mut f.db,
        rows[0],
        &[
            split::SplitLine::new(gbp(-400), Some(key("bills"))),
            split::SplitLine::new(gbp(-600), None),
        ],
    )
    .unwrap();
    f.sql(
        "UPDATE \"transaction\" SET category_id = (SELECT id FROM category WHERE key = 'food'), \
         category_source = 'preset_map' WHERE id = ?1",
        rows[1],
    );
    let before = f.snapshot(rows[1]);
    let rule = f.rule(
        "acme",
        vec![
            split_template(&[("food.groceries", 5_000), ("home", 5_000)]),
            my_share(5_000),
            set_note("n"),
        ],
    );
    let preview = f.preview(rule);
    assert_eq!(preview.split_rows, [rows[0]]);
    assert_eq!(
        preview.skipped_splits,
        [SkippedSplit {
            transaction: rows[2],
            rule,
            reason: SplitSkip::TooSmall
        }]
    );
    assert_eq!(ids(preview.automatic_rows()), [rows[1], rows[2]]);
    let batch = f.commit(&preview, &[]).batch.unwrap();
    // The split row is untouched, note included.
    assert_eq!(f.note(rows[0]), None);
    assert_eq!(f.lines(rows[0]).len(), 2);
    assert_eq!(
        f.lines(rows[1]),
        [
            ("food.groceries".to_owned(), -500, Some(-250)),
            ("home".to_owned(), -500, Some(-250)),
        ]
    );
    assert_eq!(f.category(rows[1]), (None, None, None));
    // Too small to split: the share applies to the row.
    assert_eq!(f.share(rows[2]), (Some(-1), None));

    change::undo(&mut f.db, batch).unwrap();
    assert_eq!(f.snapshot(rows[1]), before);
    assert_eq!(f.category(rows[1]).0, s("food"));
}

#[test]
fn a_split_over_a_user_value_needs_inclusion_and_transfers_arent_split() {
    let mut f = Fixture::new();
    let rows = f.import(&[
        (3, -1_000, "ACME"),
        (4, -1_000, "ACME"),
        (5, -1_000, "ACME"),
    ]);
    f.sql(
        "UPDATE \"transaction\" SET category_id = (SELECT id FROM category WHERE key = 'food'), \
         category_source = 'user' WHERE id = ?1",
        rows[0],
    );
    f.sql(
        "UPDATE \"transaction\" SET share_label = 'Sam' WHERE id = ?1",
        rows[1],
    );
    let savings = account::create(
        &mut f.db,
        &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
    )
    .unwrap();
    let mut statement = Statement::new(SourceFormat::Csv, Currency::GBP);
    statement.push(
        SourceLocation::Line(2),
        Ok(StatementLine::new(day(5), gbp(1_000), "FROM CURRENT")),
    );
    let preview = import::preview(&f.db, savings, "t.csv", statement).unwrap();
    import::commit(&mut f.db, &preview, preview.needs_confirmation()).unwrap();
    // Detection paired it with rows[2]; confirm it as a transfer.
    f.sql(
        "UPDATE transfer_group SET status = 'confirmed' \
         WHERE id = (SELECT transfer_group_id FROM \"transaction\" WHERE id = ?1)",
        rows[2],
    );
    let rule = f.rule(
        "acme",
        vec![split_template(&[("food", 5_000), ("home", 5_000)])],
    );
    let preview = f.preview(rule);
    assert_eq!(ids(preview.user_set_rows()), [rows[0], rows[1]]);
    assert_eq!(
        preview.skipped_splits,
        [SkippedSplit {
            transaction: rows[2],
            rule,
            reason: SplitSkip::InTransfer
        }]
    );
    let Target::Split { clears_share, .. } = &preview.rows[1].changes[0].new else {
        panic!("expected a split");
    };
    assert_eq!(clears_share.share_label.as_deref(), Some("Sam"));
    f.commit(&preview, &[rows[1]]);
    assert!(f.lines(rows[0]).is_empty());
    assert_eq!(f.category(rows[0]).1, s("user"));
    assert_eq!(f.lines(rows[1]).len(), 2);
    assert_eq!(f.share(rows[1]), (None, None));
}
