//! The M1 exit criteria, end to end through the public API (ROADMAP M1):
//! exports import without duplicates on re-import, reconcile where balances
//! exist and get categorised by rules; splits, tags, manual transactions and
//! refund links work through the core API; a backup exists from before every
//! import. Along the way: rule actions at import and applied to history
//! (M1.24), refunds and shared costs (M1.25), the duplicates queue (M1.26),
//! balance checks and lock dates (M1.27) and export (M1.29), and that a lock
//! holds for imports, merges, rule applies and undos.
//!
//! The data is synthetic (`fixtures/ofx/bank-sgml*.ofx`, plus rows typed in
//! here).

mod common;

use std::collections::BTreeSet;

use chrono::NaiveDate;
use coffer_core::account::{self, AccountKind, EnteredBalance, EnteredSource};
use coffer_core::backup::{self, Reason};
use coffer_core::category::CategoryKey;
use coffer_core::change::{self, Field, Value};
use coffer_core::duplicate::{self, CandidateStatus, MergeRequest};
use coffer_core::exclusion::ExclusionReason;
use coffer_core::export::{self, Filter, Format, Options};
use coffer_core::id::{AccountId, TransactionId};
use coffer_core::import::{self, Committed};
use coffer_core::link::{self, LinkKind, LinkTarget, NewLink, ShareLabel, Sharing};
use coffer_core::manual::{self, ManualTransaction};
use coffer_core::money::{Currency, Money};
use coffer_core::reconciliation::{self, Status};
use coffer_core::rule::{
    self, ActionText, AmountBounds, ExcludeReason, MatchField, MatchKind, Pattern, RuleAction,
    RuleDefinition, Share, TagList,
};
use coffer_core::split::{self, SplitLine};
use coffer_core::tag::{self, TagName};
use common::*;

fn day(month: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn key(text: &str) -> CategoryKey {
    CategoryKey::new(text).unwrap()
}

fn description_rule(text: &str, actions: Vec<RuleAction>) -> RuleDefinition {
    RuleDefinition::new(
        MatchField::Description,
        Pattern::new(MatchKind::Contains, text).unwrap(),
        AmountBounds::ANY,
        None,
        actions,
    )
    .unwrap()
}

/// The transaction with source ID `fitid` on `account`.
fn by_fitid(scratch: &Scratch, account: AccountId, fitid: &str) -> TransactionId {
    scratch
        .reader()
        .query_row(
            "SELECT id FROM \"transaction\" WHERE account_id = ?1 AND external_id = ?2",
            rusqlite::params![account.get(), fitid],
            |row| row.get::<_, i64>(0),
        )
        .map(TransactionId::new)
        .unwrap()
}

fn category_of(scratch: &Scratch, id: TransactionId) -> Option<String> {
    scratch
        .reader()
        .query_row(
            "SELECT c.key FROM \"transaction\" t LEFT JOIN category c ON c.id = t.category_id \
             WHERE t.id = ?1",
            [id.get()],
            |row| row.get(0),
        )
        .unwrap()
}

fn import_backups(scratch: &Scratch) -> usize {
    backup::list(scratch.db.path())
        .unwrap()
        .iter()
        .filter(|backup| backup.reason == Reason::Import)
        .count()
}

#[test]
fn m1_exit_criteria_end_to_end() {
    let mut scratch = Scratch::new();
    let current = scratch.current("Current", Currency::GBP);
    let cash = scratch.account("Cash", AccountKind::Cash, Currency::GBP);

    // The balance before the first export's rows, read off the bank's app:
    // the starting point the file's closing balance is checked against.
    account::add_balance(
        &mut scratch.db,
        current,
        &EnteredBalance {
            date: day(11, 1),
            balance: gbp(113_510),
            source: EnteredSource::Manual,
            note: None,
        },
    )
    .unwrap();

    // Rules before the import: field by field, every action kind.
    rule::create(
        &mut scratch.db,
        &description_rule(
            "EXAMPLE EMPLOYER",
            vec![RuleAction::SetCategory(key("income.salary"))],
        ),
    )
    .unwrap();
    rule::create(
        &mut scratch.db,
        &description_rule(
            "SANDWICH",
            vec![
                RuleAction::SetCategory(key("food.eating_out")),
                RuleAction::AddTag(TagList::new(vec![TagName::new("work").unwrap()]).unwrap()),
                RuleAction::SetNote(ActionText::new("Claim on expenses").unwrap()),
                RuleAction::SetMerchant(ActionText::new("Sandwich Example").unwrap()),
            ],
        ),
    )
    .unwrap();

    // 1. Import; a backup is taken first; rules categorise.
    assert_eq!(import_backups(&scratch), 0);
    let first = scratch.import_batch(current, "bank-sgml.ofx", read_ofx("bank-sgml.ofx"));
    assert_eq!(first.transactions, 7);
    assert_eq!(first.categorized_by_rules, 2);
    assert_eq!(import_backups(&scratch), 1);
    let salary = by_fitid(&scratch, current, "EX-0002");
    let sandwich = by_fitid(&scratch, current, "EX-0004");
    assert_eq!(
        category_of(&scratch, salary).as_deref(),
        Some("income.salary")
    );
    assert_eq!(
        category_of(&scratch, sandwich).as_deref(),
        Some("food.eating_out")
    );
    let tags: Vec<String> = tag::tags_of(&scratch.db, sandwich)
        .unwrap()
        .into_iter()
        .map(|tag| tag.name.as_str().to_owned())
        .collect();
    assert_eq!(tags, ["work"]);
    assert_eq!(
        change::read(&scratch.reader(), sandwich, Field::UserNote).unwrap(),
        Value::UserNote(Some("Claim on expenses".into()))
    );

    // 2. Re-importing stores nothing and takes no backup.
    assert_eq!(
        scratch.import(current, "bank-sgml.ofx", read_ofx("bank-sgml.ofx")),
        Committed::NothingNew
    );
    assert_eq!(import_backups(&scratch), 1);

    // 3. The file's closing balance checks out against the entered one.
    let checks = reconciliation::account_checks(&scratch.db, current).unwrap();
    assert_eq!(checks.status(), Status::Reconciled);
    assert_eq!(checks.lockable_through(), Some(day(11, 30)));
    reconciliation::lock(&mut scratch.db, current, day(11, 30)).unwrap();

    // 4. Splits, tags and a manual row through the core API, inside the
    //    lock: none of them changes a balance.
    let bakery = by_fitid(&scratch, current, "EX-0006");
    split::set(
        &mut scratch.db,
        bakery,
        &[
            SplitLine::new(gbp(-1_000), Some(key("food.groceries"))),
            SplitLine::new(gbp(-724), Some(key("food.eating_out"))),
        ],
    )
    .unwrap();
    let treat = tag::create(&mut scratch.db, &TagName::new("treat").unwrap(), false).unwrap();
    let secret = tag::create(&mut scratch.db, &TagName::new("secret").unwrap(), true).unwrap();
    tag::add_to_transaction(&mut scratch.db, bakery, treat).unwrap();
    tag::add_to_transaction(&mut scratch.db, bakery, secret).unwrap();
    // A manual row in the locked period is refused; after it, it's fine.
    assert!(
        manual::create(
            &mut scratch.db,
            &ManualTransaction::new(current, day(11, 29), gbp(-999), "Streaming"),
        )
        .is_err()
    );
    // The streaming charge, typed in a day after the bank will date it.
    let typed = manual::create(
        &mut scratch.db,
        &ManualTransaction::new(current, day(12, 2), gbp(-999), "Streaming subscription"),
    )
    .unwrap();

    // 5. A refund link: cash back from the bakery nets against the split
    //    purchase's categories, on the refund's own date.
    let refund = manual::create(
        &mut scratch.db,
        &ManualTransaction::new(cash, day(12, 4), gbp(500), "Bakery refund"),
    )
    .unwrap();
    let refund_link = link::create(
        &mut scratch.db,
        &NewLink {
            kind: LinkKind::Refund,
            inflow: refund,
            target: LinkTarget::Purchase(bakery),
            amount: gbp(500),
        },
    )
    .unwrap();
    link::confirm(&mut scratch.db, refund_link).unwrap();
    let parts: Vec<(Option<String>, i64)> = {
        let reader = scratch.reader();
        let mut statement = reader
            .prepare(
                "SELECT c.key, l.amount_minor FROM ledger_line l \
                 LEFT JOIN category c ON c.id = l.category_id \
                 WHERE l.transaction_id = ?1 AND l.link_id IS NOT NULL ORDER BY l.target_split_id",
            )
            .unwrap();
        statement
            .query_map([refund.get()], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    // 500 of 1724, pro rata: 290.02… and 209.97…, largest remainder first.
    assert_eq!(
        parts,
        [
            (Some("food.groceries".to_owned()), 290),
            (Some("food.eating_out".to_owned()), 210)
        ]
    );
    assert_eq!(
        link::purchase_left(&scratch.db, bakery).unwrap(),
        gbp(1_224)
    );

    // 6. The next export: the typed-in row is a possible duplicate of the
    //    bank's, and until it's merged the closing balance doesn't check
    //    out, naming the window since the last passing check.
    let next = scratch.import_batch(
        current,
        "bank-sgml-next.ofx",
        read_ofx("bank-sgml-next.ofx"),
    );
    assert_eq!(next.transactions, 3);
    assert_eq!(next.duplicates, 1);
    assert_eq!(import_backups(&scratch), 2);
    let streaming = by_fitid(&scratch, current, "EX-0008");
    let failing = reconciliation::account_checks(&scratch.db, current).unwrap();
    assert_eq!(failing.status(), Status::Mismatch);
    let failure = failing.failures().next().unwrap();
    assert_eq!(failure.window().unwrap().from, day(11, 30));
    assert_eq!(failure.window().unwrap().to, day(12, 31));
    assert_eq!(failure.difference_minor(), Some(999));

    let candidate = duplicate::pending(&scratch.db, Some(current)).unwrap();
    assert_eq!(candidate.len(), 1);
    let candidate = candidate[0].id;
    // The bank's row has a source ID, so it's kept by default.
    assert_eq!(
        duplicate::default_keep(&scratch.db, candidate).unwrap(),
        streaming
    );
    let preview = duplicate::merge_preview(&scratch.db, &[MergeRequest::new(candidate)]).unwrap();
    assert!(!preview.is_locked());
    let merged = duplicate::merge(&mut scratch.db, &preview).unwrap();
    match change::read(&scratch.reader(), typed, Field::Exclusion).unwrap() {
        Value::Exclusion(Some(exclusion)) => {
            assert_eq!(exclusion.reason, ExclusionReason::Duplicate);
        }
        other => panic!("not excluded: {other:?}"),
    }
    // Merged, never deleted; and now the balance checks out.
    assert!(manual::get(&scratch.db, typed).is_ok());
    let checks = reconciliation::account_checks(&scratch.db, current).unwrap();
    assert_eq!(checks.status(), Status::Reconciled);

    // Undoing the merge brings the row back, and the candidate to pending.
    change::undo(&mut scratch.db, merged.batch).unwrap();
    assert_eq!(
        duplicate::get(&scratch.db, candidate).unwrap().status,
        CandidateStatus::Pending
    );
    assert_eq!(
        reconciliation::account_checks(&scratch.db, current)
            .unwrap()
            .status(),
        Status::Mismatch
    );
    let preview = duplicate::merge_preview(&scratch.db, &[MergeRequest::new(candidate)]).unwrap();
    let merged = duplicate::merge(&mut scratch.db, &preview).unwrap();

    // 7. Shared costs: half the groceries are Sam's until Sam pays back.
    let groceries = by_fitid(&scratch, current, "EX-0010");
    let sam = ShareLabel::new("Sam").unwrap();
    link::set_sharing(
        &mut scratch.db,
        groceries,
        &Sharing::Shared {
            my_share: gbp(-3_225),
            label: sam.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        link::owed_to(&scratch.db, &sam, Currency::GBP).unwrap(),
        gbp(3_225)
    );
    let paid_back = manual::create(
        &mut scratch.db,
        &ManualTransaction::new(cash, day(12, 12), gbp(3_225), "Settling up"),
    )
    .unwrap();
    let settlement = link::create(
        &mut scratch.db,
        &NewLink {
            kind: LinkKind::Settlement,
            inflow: paid_back,
            target: LinkTarget::Person(sam.clone()),
            amount: gbp(3_225),
        },
    )
    .unwrap();
    link::confirm(&mut scratch.db, settlement).unwrap();
    assert_eq!(
        link::owed_to(&scratch.db, &sam, Currency::GBP).unwrap(),
        gbp(0)
    );

    // 8. A rule applied to history, as one undoable change batch; an
    //    exclusion it would make inside the lock needs an unlock.
    let streaming_rule = rule::create(
        &mut scratch.db,
        &description_rule(
            "STREAMING",
            vec![RuleAction::SetCategory(key("bills.subscriptions"))],
        ),
    )
    .unwrap();
    let preview = rule::preview_apply(&scratch.db, streaming_rule).unwrap();
    let applied = rule::commit_apply(&mut scratch.db, &preview, &BTreeSet::new()).unwrap();
    assert_eq!(
        category_of(&scratch, streaming).as_deref(),
        Some("bills.subscriptions")
    );
    change::undo(&mut scratch.db, applied.batch.unwrap()).unwrap();
    assert_eq!(category_of(&scratch, streaming), None);

    let dispute = rule::create(
        &mut scratch.db,
        &description_rule("ENERGY", vec![RuleAction::Exclude(ExcludeReason::Disputed)]),
    )
    .unwrap();
    let preview = rule::preview_apply(&scratch.db, dispute).unwrap();
    assert_eq!(preview.locked_rows().count(), 1);
    assert!(matches!(
        rule::commit_apply(&mut scratch.db, &preview, &BTreeSet::new()),
        Err(rule::ApplyError::Locked(_))
    ));

    // 9. The lock holds for undos too: the first import, and the merge once
    //    its row is locked.
    assert!(matches!(
        import::undo(&mut scratch.db, first.batch),
        Err(import::Error::Locked { .. })
    ));
    reconciliation::lock(&mut scratch.db, current, day(12, 31)).unwrap();
    assert!(matches!(
        change::undo(&mut scratch.db, merged.batch),
        Err(change::Error::Locked { .. })
    ));
    let rows_before = scratch.stored(current).len();
    assert!(matches!(
        import::undo(&mut scratch.db, next.batch),
        Err(import::Error::Locked { .. })
    ));
    assert_eq!(scratch.stored(current).len(), rows_before);

    // With an explicit unlock, the rule's exclusion goes through.
    reconciliation::unlock(&mut scratch.db, current).unwrap();
    let preview = rule::preview_apply(&scratch.db, dispute).unwrap();
    let excluded = rule::commit_apply(&mut scratch.db, &preview, &BTreeSet::new()).unwrap();
    assert_eq!(excluded.transactions, 1);
    change::undo(&mut scratch.db, excluded.batch.unwrap()).unwrap();

    // 10. Export: split lines keep their parent ID, link parts say which
    //     link, and private tags and notes stay out unless asked for.
    let mut out = Vec::new();
    export::write(&scratch.db, Format::Json, &Options::default(), &mut out).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let rows = json["rows"].as_array().unwrap();
    let bakery_lines: Vec<&serde_json::Value> = rows
        .iter()
        .filter(|row| row["transaction_id"] == bakery.get() && !row["split_id"].is_null())
        .collect();
    assert_eq!(bakery_lines.len(), 2);
    assert_eq!(bakery_lines[0]["tags"], serde_json::json!(["treat"]));
    let refund_parts = rows
        .iter()
        .filter(|row| row["transaction_id"] == refund.get() && !row["link_id"].is_null())
        .count();
    assert_eq!(refund_parts, 2);
    let text = String::from_utf8(out).unwrap();
    assert!(!text.contains("secret"));
    assert!(!text.contains("Claim on expenses"));
    assert!(!text.contains("Sam"));
    let mut with_notes = Vec::new();
    export::write(
        &scratch.db,
        Format::Csv,
        &Options {
            filter: Filter {
                tags: vec![TagName::new("secret").unwrap()],
                ..Filter::default()
            },
            include_notes: true,
            include_private_tags: true,
            include_excluded: true,
        },
        &mut with_notes,
    )
    .unwrap();
    let with_notes = String::from_utf8(with_notes).unwrap();
    assert!(with_notes.contains("secret"));
    assert_eq!(
        with_notes.lines().count(),
        3,
        "a header and two split lines"
    );

    // 11. Undoing the next import clears its candidates and links, and says
    //     a row merged into it stays excluded.
    let refund_on_groceries = manual::create(
        &mut scratch.db,
        &ManualTransaction::new(cash, day(12, 14), gbp(450), "Groceries refund"),
    )
    .unwrap();
    link::create(
        &mut scratch.db,
        &NewLink {
            kind: LinkKind::Refund,
            inflow: refund_on_groceries,
            target: LinkTarget::Purchase(groceries),
            amount: gbp(450),
        },
    )
    .unwrap();
    let undo = import::undo_preview(&scratch.db, next.batch).unwrap();
    assert_eq!(undo.merged_into, 1);
    import::undo(&mut scratch.db, next.batch).unwrap();
    assert!(
        duplicate::of_transaction(&scratch.db, typed)
            .unwrap()
            .is_empty()
    );
    assert!(
        link::links_of(&scratch.db, refund_on_groceries)
            .unwrap()
            .is_empty()
    );
    // The bakery's link and the settlement aren't the batch's: they stay.
    assert_eq!(link::links_of(&scratch.db, refund).unwrap().len(), 1);
    assert_eq!(link::links_of(&scratch.db, paid_back).unwrap().len(), 1);
}

#[test]
fn a_rule_share_or_split_skips_linked_money_in() {
    let mut scratch = Scratch::new();
    let cash = scratch.account("Cash", AccountKind::Cash, Currency::GBP);
    let purchase = manual::create(
        &mut scratch.db,
        &ManualTransaction::new(cash, day(11, 3), gbp(-2_000), "ACME order"),
    )
    .unwrap();
    let refund = manual::create(
        &mut scratch.db,
        &ManualTransaction::new(cash, day(11, 9), gbp(2_000), "ACME refund"),
    )
    .unwrap();
    link::create(
        &mut scratch.db,
        &NewLink {
            kind: LinkKind::Refund,
            inflow: refund,
            target: LinkTarget::Purchase(purchase),
            amount: gbp(2_000),
        },
    )
    .unwrap();
    let halves = rule::create(
        &mut scratch.db,
        &description_rule(
            "ACME",
            vec![RuleAction::MyShare(Share::new(5_000).unwrap())],
        ),
    )
    .unwrap();
    let preview = rule::preview_apply(&scratch.db, halves).unwrap();
    // The purchase takes the share; the linked refund is left alone.
    let rows: Vec<TransactionId> = preview.rows.iter().map(|row| row.transaction).collect();
    assert_eq!(rows, [purchase]);
    rule::commit_apply(&mut scratch.db, &preview, &BTreeSet::new()).unwrap();

    // A bulk change can't give it one either.
    let mut conn = scratch.writer();
    let tx = conn.transaction().unwrap();
    let mut recorder = change::Recorder::new(change::ChangeKind::BulkEdit);
    assert!(matches!(
        recorder.set(
            &tx,
            refund,
            Value::Share(change::ShareValue {
                my_share_minor: Some(1_000),
                share_label: None,
            }),
        ),
        Err(change::Error::Unwritable { .. })
    ));
}
