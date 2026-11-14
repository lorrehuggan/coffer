//! UI integration tests for the review queues: the shell's Inbox with its
//! CHECKS in a headless window, against a scratch database seeded with
//! [`fixture`] through [`ChecksStore`], as `coffer` runs it.

use chrono::NaiveDate;
use coffer_core::db::Database;
use coffer_core::duplicate::{self, CandidateStatus};
use coffer_core::exclusion::{self, ExclusionReason};
use coffer_core::reconciliation;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use tempfile::TempDir;

use super::*;
use crate::accounts::{Accounts, fixture as accounts_fixture};
use crate::inbox::Inbox;
use crate::shell::{ShellView, View};

struct Scratch {
    _dir: TempDir,
    path: std::path::PathBuf,
    seeded: fixture::Seeded,
}

impl Scratch {
    fn new(locked: bool) -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("coffer.db");
        let mut db = Database::open(&path).unwrap();
        let seeded = fixture::seed(&mut db);
        if locked {
            fixture::seed_locked_pair(&mut db, &seeded);
        }
        Self {
            _dir: dir,
            path,
            seeded,
        }
    }

    fn db(&self) -> Database {
        Database::open(&self.path).unwrap()
    }

    fn status(&self, id: DuplicateCandidateId) -> CandidateStatus {
        duplicate::get(&self.db(), id).unwrap().status
    }

    fn excluded(&self, id: TransactionId) -> Option<ExclusionReason> {
        exclusion::get(&self.db(), id)
            .unwrap()
            .map(|exclusion| exclusion.reason)
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
) -> (Entity<ShellView>, Entity<Checks>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let store = ChecksStore::new(scratch.path.clone());
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
        let inbox = cx.new(|_| Inbox::empty());
        let mut shell = ShellView::with_data(View::Inbox, accounts, inbox, window, cx);
        let checks = cx.new(|_| Checks::from_store(store));
        shell.install_checks(checks, window, cx);
        shell
    });
    let checks = cx.update(|_, cx| shell.read(cx).checks().clone());
    redraw(cx);
    (shell, checks, cx)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, cx| window.click(id, cx));
    redraw(cx);
}

fn shows(id: &str, cx: &mut VisualTestContext) -> bool {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, _| window.try_find(id).is_some())
}

fn selected_pair(checks: &Entity<Checks>, cx: &mut VisualTestContext) -> Option<Pair> {
    cx.update(|_, cx| checks.read(cx).selected_pair().cloned())
}

/// The pending pair on `account`.
fn pair_on(checks: &Entity<Checks>, account: AccountId, cx: &mut VisualTestContext) -> Pair {
    cx.update(|_, cx| {
        checks
            .read(cx)
            .pairs()
            .iter()
            .find(|pair| pair.account == account)
            .cloned()
            .expect("a pair on the account")
    })
}

#[gpui_kit::test]
fn checks_lists_the_queues_and_opens_the_first_pair(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (shell, checks, cx) = open(cx, &scratch);
    let (pairs, failing) = cx.update(|_, cx| {
        let checks = checks.read(cx);
        (checks.pending_pairs(), checks.failing())
    });
    assert_eq!(pairs, 3, "Ocado, Trainline and Pret");
    assert_eq!(failing, 1, "the joint account's 9 Nov");
    assert!(shows("inbox-checks-duplicates", cx));

    click("inbox-checks-duplicates", cx);
    let queue = cx.update(|_, cx| checks.read(cx).queue());
    assert_eq!(queue, Some(Queue::Duplicates));
    let pair = selected_pair(&checks, cx).expect("the first pair opens");
    assert_eq!(pair.account, scratch.seeded.joint);
    assert_eq!(text::name(&pair.rows[0]), "OCADO RETAIL LTD HATFIELD GB");
    assert!(shows("checks-merge", cx), "the pair is open in place");

    // A suggestion row goes back to the suggestions.
    click("inbox-status-pending", cx);
    let queue = cx.update(|_, cx| checks.read(cx).queue());
    assert_eq!(queue, None);
    assert_eq!(cx.update(|_, cx| shell.read(cx).view()), View::Inbox);
}

#[gpui_kit::test]
fn merging_excludes_the_copy_fixes_the_check_and_undoes(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (_shell, checks, cx) = open(cx, &scratch);
    click("inbox-checks-duplicates", cx);
    let pair = selected_pair(&checks, cx).unwrap();
    let excluded = pair.rows[1].transaction;
    let effect = pair.effect.clone().unwrap();
    assert!(effect.checks.iter().any(|change| change.fixes()));

    cx.simulate_keystrokes("m");
    redraw(cx);
    assert_eq!(scratch.status(pair.candidate), CandidateStatus::Merged);
    assert_eq!(scratch.excluded(excluded), Some(ExclusionReason::Duplicate));
    let (failing, outcome, next) = cx.update(|_, cx| {
        let checks = checks.read(cx);
        (
            checks.failing(),
            checks.outcome().cloned(),
            checks.selected_pair().map(|pair| pair.candidate),
        )
    });
    assert_eq!(failing, 0, "the gap is closed");
    assert!(matches!(
        outcome,
        Some(Outcome::Done {
            undo: Some(Undo::Merge(..)),
            ..
        })
    ));
    assert_ne!(next, Some(pair.candidate), "the next pair opens");

    click("undo-change", cx);
    assert_eq!(scratch.status(pair.candidate), CandidateStatus::Pending);
    assert_eq!(scratch.excluded(excluded), None);
    let (failing, selected) = cx.update(|_, cx| {
        let checks = checks.read(cx);
        (
            checks.failing(),
            checks.selected_pair().map(|pair| pair.candidate),
        )
    });
    assert_eq!(failing, 1);
    assert_eq!(selected, Some(pair.candidate), "the pair opens again");
}

#[gpui_kit::test]
fn keep_this_instead_swaps_the_rows(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (_shell, checks, cx) = open(cx, &scratch);
    click("inbox-checks-duplicates", cx);
    let pair = selected_pair(&checks, cx).unwrap();
    let (kept, other) = (pair.rows[0].transaction, pair.rows[1].transaction);

    cx.simulate_keystrokes("left");
    redraw(cx);
    let swapped = selected_pair(&checks, cx).unwrap();
    assert_eq!(swapped.default_keep(), other);
    assert_eq!(swapped.effect.as_ref().unwrap().excluded, kept);

    click("checks-keep-other", cx);
    assert_eq!(selected_pair(&checks, cx).unwrap().default_keep(), kept);

    cx.simulate_keystrokes("right m");
    redraw(cx);
    assert_eq!(scratch.excluded(kept), Some(ExclusionReason::Duplicate));
    assert_eq!(scratch.excluded(other), None);
}

#[gpui_kit::test]
fn not_duplicates_dismisses_and_undoes(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (_shell, checks, cx) = open(cx, &scratch);
    click("inbox-checks-duplicates", cx);
    let pair = selected_pair(&checks, cx).unwrap();
    click("checks-dismiss", cx);
    assert_eq!(scratch.status(pair.candidate), CandidateStatus::Dismissed);
    assert_eq!(cx.update(|_, cx| checks.read(cx).pending_pairs()), 2);
    click("undo-change", cx);
    assert_eq!(scratch.status(pair.candidate), CandidateStatus::Pending);
    assert_eq!(cx.update(|_, cx| checks.read(cx).pending_pairs()), 3);
}

#[gpui_kit::test]
fn a_locked_duplicate_needs_an_unlock(cx: &mut TestAppContext) {
    let scratch = Scratch::new(true);
    let (_shell, checks, cx) = open(cx, &scratch);
    click("inbox-checks-duplicates", cx);
    let pair = pair_on(&checks, scratch.seeded.savings, cx);
    click(&format!("checks-pair-{}", pair.candidate), cx);
    let effect = selected_pair(&checks, cx).unwrap().effect.unwrap();
    assert!(effect.locked.is_some());
    assert!(effect.checks.iter().any(|change| change.breaks()));
    assert!(!cx.update(|_, cx| checks.read(cx).can_merge()));

    // M does nothing while it's locked.
    cx.simulate_keystrokes("m");
    redraw(cx);
    assert_eq!(scratch.status(pair.candidate), CandidateStatus::Pending);

    // The block's Unlock… opens the unlock dialog; unlocking lets it merge.
    click("checks-pair-unlock", cx);
    assert!(shows("unlock-confirm", cx));
    click("unlock-confirm", cx);
    assert_eq!(
        reconciliation::locked_through(&scratch.db(), scratch.seeded.savings).unwrap(),
        None
    );
    assert!(cx.update(|_, cx| checks.read(cx).can_merge()));
}

#[gpui_kit::test]
fn balance_checks_open_the_failure_and_review_its_pair(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (_shell, checks, cx) = open(cx, &scratch);
    click("inbox-checks-balances", cx);
    let (account, explained) = cx.update(|_, cx| {
        let checks = checks.read(cx);
        let (account, row) = checks.selected_check().expect("the failing check");
        assert!(row.check.outcome.is_failed());
        (account.account, row.explained_by)
    });
    assert_eq!(account, scratch.seeded.joint);
    let explained = explained.expect("the Ocado pair explains it");
    // Failing accounts first.
    let first = cx.update(|_, cx| checks.read(cx).accounts()[0].account);
    assert_eq!(first, scratch.seeded.joint);

    click("checks-review-pair", cx);
    let (queue, pair) = cx.update(|_, cx| {
        let checks = checks.read(cx);
        (
            checks.queue(),
            checks.selected_pair().map(|pair| pair.candidate),
        )
    });
    assert_eq!(queue, Some(Queue::Duplicates));
    assert_eq!(pair, Some(explained));
}

#[gpui_kit::test]
fn lock_and_unlock_through_the_dialogs(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let savings = scratch.seeded.savings;
    let (_shell, checks, cx) = open(cx, &scratch);
    click("inbox-checks-balances", cx);
    click(&format!("checks-lock-{savings}"), cx);
    assert!(shows("lock-confirm", cx));
    click("lock-confirm", cx);
    let through = NaiveDate::from_ymd_opt(2026, 10, 31).unwrap();
    assert_eq!(
        reconciliation::locked_through(&scratch.db(), savings).unwrap(),
        Some(through),
        "the latest passing check"
    );
    assert!(!shows("lock-confirm", cx), "the dialog closes");
    let lockable = cx.update(|_, cx| checks.read(cx).account(savings).unwrap().lockable.clone());
    assert!(lockable.is_empty(), "nothing later to lock");

    cx.update(|_, cx| checks.update(cx, |checks, cx| checks.request_unlock(savings, cx)));
    redraw(cx);
    click("unlock-confirm", cx);
    assert_eq!(
        reconciliation::locked_through(&scratch.db(), savings).unwrap(),
        None
    );
}

#[gpui_kit::test]
fn the_flagged_dot_opens_the_accounts_failing_check(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let joint = scratch.seeded.joint;
    let (shell, checks, cx) = open(cx, &scratch);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.show(View::Transactions, window, cx))
    });
    let accounts = cx.update(|_, cx| shell.read(cx).accounts().clone());
    cx.update(|_, cx| accounts.update(cx, |accounts, cx| accounts.request_checks(joint, cx)));
    redraw(cx);
    assert_eq!(cx.update(|_, cx| shell.read(cx).view()), View::Inbox);
    let (queue, selected) = cx.update(|_, cx| {
        let checks = checks.read(cx);
        (
            checks.queue(),
            checks
                .selected_check()
                .map(|(account, row)| (account.account, row.check.outcome.is_failed())),
        )
    });
    assert_eq!(queue, Some(Queue::Balances));
    assert_eq!(selected, Some((joint, true)));
}

#[gpui_kit::test]
fn the_status_bar_counts_failing_checks(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (shell, checks, cx) = open(cx, &scratch);
    assert!(shows("status-balance-checks", cx));
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.show(View::Transactions, window, cx))
    });
    redraw(cx);
    click("status-balance-checks", cx);
    let queue = cx.update(|_, cx| checks.read(cx).queue());
    assert_eq!(queue, Some(Queue::Balances));
    assert_eq!(cx.update(|_, cx| shell.read(cx).view()), View::Inbox);
}

#[test]
fn step_in_stops_at_the_ends() {
    let order = [1, 2, 3];
    assert_eq!(step_in(&order, None, 1), Some(1));
    assert_eq!(step_in(&order, None, -1), Some(3));
    assert_eq!(step_in(&order, Some(3), 1), Some(3));
    assert_eq!(step_in(&order, Some(2), -1), Some(1));
    assert_eq!(step_in::<i32>(&[], None, 1), None);
}
