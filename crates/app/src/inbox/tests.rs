//! UI integration tests for the Inbox: the shell's Inbox view in a
//! headless window, against a scratch database seeded with [`fixture`]
//! through [`InboxStore`], as `coffer` runs it.

use coffer_core::db::Database;
use coffer_core::inbox::{self as core_inbox, Filter, Payload, SuggestionId, SuggestionStatus};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use tempfile::TempDir;

use super::*;
use crate::accounts::{Accounts, fixture as accounts_fixture};
use crate::shell::{ShellView, View};

struct Scratch {
    _dir: TempDir,
    path: std::path::PathBuf,
}

impl Scratch {
    fn new(wide: bool) -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("coffer.db");
        let mut db = Database::open(&path).unwrap();
        fixture::seed(&mut db);
        if wide {
            fixture::seed_wide_rule(&mut db);
        }
        Self { _dir: dir, path }
    }

    fn db(&self) -> Database {
        Database::open(&self.path).unwrap()
    }

    fn status(&self, id: SuggestionId) -> SuggestionStatus {
        core_inbox::get(&self.db(), id).unwrap().status
    }

    /// The pending suggestion `pick` finds.
    fn find(&self, pick: impl Fn(&Payload) -> bool) -> SuggestionId {
        core_inbox::list(
            &self.db(),
            &Filter {
                status: Some(SuggestionStatus::Pending),
                kind: None,
            },
        )
        .unwrap()
        .into_iter()
        .find(|s| pick(&s.payload))
        .expect("the fixture has it")
        .id
    }

    fn rule(&self, pattern: &str) -> SuggestionId {
        self.find(
            |payload| matches!(payload, Payload::Rule(rule) if rule.pattern().as_str() == pattern),
        )
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
) -> (Entity<ShellView>, Entity<Inbox>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let store = InboxStore::new(scratch.path.clone());
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
        let inbox = cx.new(|_| Inbox::from_store(store));
        ShellView::with_data(View::Inbox, accounts, inbox, window, cx)
    });
    let inbox = cx.update(|_, cx| shell.read(cx).inbox().clone());
    redraw(cx);
    (shell, inbox, cx)
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

fn selected(inbox: &Entity<Inbox>, cx: &mut VisualTestContext) -> Option<SuggestionId> {
    cx.update(|_, cx| inbox.read(cx).selected())
}

#[gpui_kit::test]
fn groups_follow_the_kind_order_and_the_badge_leaves_coffer_out(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (_shell, inbox, cx) = open(cx, &scratch);
    let (kinds, badge, pending) = cx.update(|_, cx| {
        let inbox = inbox.read(cx);
        let kinds: Vec<(SuggestionKind, String)> = inbox
            .groups()
            .into_iter()
            .map(|(key, _)| (key.kind, key.source))
            .collect();
        (kinds, inbox.badge(), inbox.data().counts.pending)
    });
    let agent = fixture::AGENT.to_owned();
    assert_eq!(
        kinds,
        vec![
            (SuggestionKind::Rule, agent.clone()),
            (SuggestionKind::Rule, COFFER_SOURCE.to_owned()),
            (SuggestionKind::MerchantAlias, agent.clone()),
            (SuggestionKind::TransferPair, agent.clone()),
            (SuggestionKind::RefundLink, agent.clone()),
            (SuggestionKind::Categorization, agent),
        ]
    );
    assert_eq!(pending, 15);
    assert_eq!(badge, 14, "Coffer's own suggestion isn't counted");
}

#[gpui_kit::test]
fn kind_and_source_filter_and_a_second_click_clears(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (_shell, inbox, cx) = open(cx, &scratch);
    click("inbox-kind-rule", cx);
    let shown = cx.update(|_, cx| inbox.read(cx).order().len());
    assert_eq!(shown, 3, "two agent rules and Coffer's");
    click(&format!("inbox-source-{COFFER_SOURCE}"), cx);
    let shown = cx.update(|_, cx| inbox.read(cx).order().len());
    assert_eq!(shown, 1);
    click("inbox-kind-rule", cx);
    click(&format!("inbox-source-{COFFER_SOURCE}"), cx);
    let shown = cx.update(|_, cx| inbox.read(cx).order().len());
    assert_eq!(shown, 15);
}

#[gpui_kit::test]
fn a_click_opens_a_rule_and_its_check_approves_it_with_history(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let uber = scratch.rule("UBER *EATS");
    let (_shell, inbox, cx) = open(cx, &scratch);
    click(&format!("inbox-row-{uber}"), cx);
    assert_eq!(selected(&inbox, cx), Some(uber));
    assert!(
        shows(&format!("inbox-approve-{uber}"), cx),
        "✓ shows on the selected row"
    );
    assert!(shows("inbox-card-approve", cx));

    click(&format!("inbox-approve-{uber}"), cx);
    assert_eq!(scratch.status(uber), SuggestionStatus::Approved);
    // Applied to past transactions, settling the UBER categorisation.
    let approved = core_inbox::get(&scratch.db(), uber).unwrap();
    assert!(approved.change_batch.is_some(), "applied to history");
    let outcome = cx.update(|_, cx| inbox.read(cx).outcome().cloned());
    assert_eq!(
        outcome,
        Some(Outcome::Done(
            "Approved rule UBER *EATS: 5 transactions now Eating out".into()
        ))
    );
    assert_ne!(selected(&inbox, cx), Some(uber), "the selection moves on");
}

#[gpui_kit::test]
fn keys_move_approve_and_reject(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let deliveroo = scratch.rule("DELIVEROO");
    let uber = scratch.rule("UBER *EATS");
    let (_shell, inbox, cx) = open(cx, &scratch);
    cx.simulate_keystrokes("down");
    assert_eq!(selected(&inbox, cx), Some(deliveroo));
    cx.simulate_keystrokes("down");
    assert_eq!(selected(&inbox, cx), Some(uber));
    cx.simulate_keystrokes("r");
    redraw(cx);
    assert_eq!(scratch.status(uber), SuggestionStatus::Rejected);
    cx.simulate_keystrokes("up a");
    redraw(cx);
    assert_eq!(scratch.status(deliveroo), SuggestionStatus::Approved);
    cx.simulate_keystrokes("escape");
    assert_eq!(selected(&inbox, cx), None);
}

#[gpui_kit::test]
fn approve_all_leaves_a_wide_rule_out(cx: &mut TestAppContext) {
    let scratch = Scratch::new(true);
    let wide = scratch.rule("PAY");
    let (_shell, inbox, cx) = open(cx, &scratch);
    let group = GroupKey {
        kind: SuggestionKind::Rule,
        source: fixture::AGENT.to_owned(),
    };
    let approvable = cx.update(|_, cx| inbox.read(cx).approvable(&group));
    assert_eq!(approvable.len(), 2);
    assert!(!approvable.contains(&wide));
    click(&format!("inbox-approve-rule-{}", fixture::AGENT), cx);
    assert_eq!(scratch.status(wide), SuggestionStatus::Pending);
    assert_eq!(scratch.status(approvable[0]), SuggestionStatus::Approved);
    // Its own ✓ still approves it.
    cx.update(|_, cx| inbox.update(cx, |inbox, cx| inbox.approve(wide, cx)));
    assert_eq!(scratch.status(wide), SuggestionStatus::Approved);
}

#[gpui_kit::test]
fn reject_all_rejects_the_group(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (_shell, inbox, cx) = open(cx, &scratch);
    click(
        &format!("inbox-reject-categorization-{}", fixture::AGENT),
        cx,
    );
    let counts = cx.update(|_, cx| inbox.read(cx).data().counts.clone());
    assert_eq!(counts.rejected, 9);
    assert_eq!(
        counts.pending_by_kind.get(&SuggestionKind::Categorization),
        None
    );
    // Rejected lists them, read-only.
    click("inbox-status-rejected", cx);
    let (status, shown) = cx.update(|_, cx| {
        let inbox = inbox.read(cx);
        (inbox.status(), inbox.order().len())
    });
    assert_eq!((status, shown), (SuggestionStatus::Rejected, 9));
    assert!(!shows(
        &format!("inbox-reject-categorization-{}", fixture::AGENT),
        cx
    ));
}

#[gpui_kit::test]
fn a_refused_approval_says_why_and_changes_nothing(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let pair = scratch.find(|payload| matches!(payload, Payload::TransferPair(_)));
    let (_shell, inbox, cx) = open(cx, &scratch);
    // A side goes after the Inbox read it.
    let Payload::TransferPair(sides) = core_inbox::get(&scratch.db(), pair).unwrap().payload else {
        unreachable!()
    };
    coffer_core::manual::delete(&mut scratch.db(), sides.outflow).unwrap();
    cx.update(|_, cx| inbox.update(cx, |inbox, cx| inbox.approve(pair, cx)));
    assert_eq!(scratch.status(pair), SuggestionStatus::Pending);
    let outcome = cx.update(|_, cx| inbox.read(cx).outcome().cloned());
    assert!(matches!(
        outcome,
        Some(Outcome::Refused { scope: Scope::Suggestion(id), .. }) if id == pair
    ));
}
