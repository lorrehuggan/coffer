//! UI integration tests for the Transactions view: selection and keys on the
//! Paper fixture, and filters, sorting and category edits against a scratch
//! database through [`TransactionStore`], as `coffer` runs them.

use chrono::NaiveDate;
use coffer_core::account::{self, AccountKind, NewAccount};
use coffer_core::db::Database;
use coffer_core::id::{AccountId, TransactionId};
use coffer_core::manual::{self, ManualTransaction};
use coffer_core::money::{Currency, Money};
use coffer_core::reporting::register::RowCategory;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{Modifiers, SharedString, TestAppContext, VisualTestContext};
use tempfile::TempDir;

use super::*;

fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        init(cx);
    });
}

/// The view over the Paper fixture, in a window.
pub(super) fn fixture_view(cx: &mut TestAppContext) -> Entity<TransactionsView> {
    setup(cx);
    let (view, _) =
        cx.add_window_view(|window, cx| TransactionsView::new(Source::Fixture, window, cx));
    view
}

fn open_fixture(cx: &mut TestAppContext) -> (Entity<TransactionsView>, &mut VisualTestContext) {
    setup(cx);
    let (view, cx) =
        cx.add_window_view(|window, cx| TransactionsView::new(Source::Fixture, window, cx));
    redraw(cx);
    (view, cx)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn click(id: impl Into<SharedString>, cx: &mut VisualTestContext) {
    let id = id.into();
    cx.update(move |window, cx| window.click(id, cx));
    redraw(cx);
}

fn shows(id: impl Into<SharedString>, cx: &mut VisualTestContext) -> bool {
    let id = id.into();
    cx.update(move |window, _| window.try_find(id).is_some())
}

fn type_text(text: &str, cx: &mut VisualTestContext) {
    let text = text.to_owned();
    cx.update(move |window, cx| window.input(&text, cx));
    redraw(cx);
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    redraw(cx);
}

fn row_id(id: TransactionId) -> String {
    format!("txn-{id}")
}

fn selected(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) -> Vec<TransactionId> {
    cx.update(|_, cx| view.read(cx).selected().iter().map(|row| row.id).collect())
}

fn listed(view: &Entity<TransactionsView>, cx: &mut VisualTestContext) -> Vec<TransactionId> {
    cx.update(|_, cx| {
        let view = view.read(cx);
        view.listing()
            .rows
            .iter()
            .map(|&index| view.register().rows()[index].id)
            .collect()
    })
}

// The fixture ----------------------------------------------------------------

#[gpui_kit::test]
fn clicks_replace_extend_and_toggle(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    let id = fixture::id;
    click(row_id(id("txn-pret-09")), cx);
    assert_eq!(selected(&view, cx), [id("txn-pret-09")]);

    cx.update(|window, cx| {
        window.click_with_modifiers(row_id(id("txn-tfl-09")), Modifiers::shift(), cx)
    });
    redraw(cx);
    assert_eq!(selected(&view, cx).len(), 5, "⇧-click selects the range");

    cx.update(|window, cx| {
        let toggle = Modifiers::secondary_key(cx);
        window.click_with_modifiers(row_id(id("txn-rainy-day-09")), toggle, cx)
    });
    redraw(cx);
    assert_eq!(
        selected(&view, cx),
        [
            id("txn-pret-09"),
            id("txn-bluebottle-09"),
            id("txn-amazon-09"),
            id("txn-tfl-09")
        ],
        "⌘-click toggles one row"
    );
}

#[gpui_kit::test]
fn keys_move_and_extend_the_selection(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    let id = fixture::id;
    // A click focuses the table, which the keys need.
    click(row_id(id("txn-amazon-09")), cx);
    press("down", cx);
    assert_eq!(selected(&view, cx), [id("txn-tfl-09")]);

    press("shift-up shift-up", cx);
    assert_eq!(
        selected(&view, cx),
        [
            id("txn-rainy-day-09"),
            id("txn-amazon-09"),
            id("txn-tfl-09")
        ]
    );
    assert!(
        shows("bulk-clear", cx),
        "several rows show the bulk-action bar"
    );

    press("escape", cx);
    assert!(selected(&view, cx).is_empty());
    assert!(!shows("bulk-clear", cx));

    press("end", cx);
    let last = fixture::TRANSACTIONS.last().expect("fixture rows").id;
    assert_eq!(selected(&view, cx), [id(last)]);
}

#[gpui_kit::test]
fn search_narrows_the_rows_and_escape_clears_it(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    click("search", cx);
    type_text("pret", cx);
    let rows = listed(&view, cx);
    assert_eq!(rows.len(), 3, "every Pret A Manger row");
    assert!(cx.update(|_, cx| {
        let register = view.read(cx).register();
        rows.iter().all(|id| {
            register
                .row(*id)
                .unwrap()
                .description
                .to_lowercase()
                .contains("pret")
        })
    }));

    // Nothing matches: the empty state offers to clear the filters.
    type_text("zzz", cx);
    assert!(listed(&view, cx).is_empty());

    // The filter hid the selected row, so nothing is selected: Escape
    // clears the filters.
    assert!(selected(&view, cx).is_empty());
    press("escape", cx);
    assert_eq!(listed(&view, cx).len(), fixture::TRANSACTIONS.len());
    assert!(cx.update(|_, cx| view.read(cx).filter().text.is_empty()));
}

#[gpui_kit::test]
fn uncategorised_toggles_and_sort_reorders_within_days(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    click("filter-uncategorised", cx);
    let rows = listed(&view, cx);
    assert!(!rows.is_empty());
    assert!(cx.update(|_, cx| {
        let view = view.read(cx);
        rows.iter()
            .all(|id| view.register().row(*id).unwrap().is_uncategorised())
    }));
    click("filter-uncategorised", cx);
    assert_eq!(listed(&view, cx).len(), fixture::TRANSACTIONS.len());

    // Amount, smallest (most money out) first, within each day.
    click("sort-amount", cx);
    let rows = listed(&view, cx);
    assert_eq!(rows[0], fixture::id("txn-rainy-day-09"));
    click("sort-amount", cx);
    assert_eq!(listed(&view, cx)[0], fixture::id("txn-tfl-09"));
    click("sort-amount", cx);
    assert_eq!(
        listed(&view, cx)[0],
        fixture::id("txn-pret-09"),
        "back to newest first"
    );
}

#[gpui_kit::test]
fn the_date_chip_filters_by_period(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    click("filter-date", cx);
    assert_eq!(
        cx.update(|_, cx| view.read(cx).open_menu()),
        Some(Menu::Date)
    );
    click("option-last_month", cx);
    assert!(listed(&view, cx).is_empty(), "the fixture is all November");
    assert_eq!(cx.update(|_, cx| view.read(cx).open_menu()), None);

    click("filter-date", cx);
    press("up", cx);
    press("enter", cx);
    assert_eq!(
        listed(&view, cx).len(),
        fixture::TRANSACTIONS.len(),
        "All time"
    );
}

#[gpui_kit::test]
fn the_fixture_cant_be_changed(cx: &mut TestAppContext) {
    let (view, cx) = open_fixture(cx);
    click(row_id(fixture::id("txn-bluebottle-09")), cx);
    press("enter", cx);
    assert_eq!(
        cx.update(|_, cx| view.read(cx).open_menu()),
        Some(Menu::Row(fixture::id("txn-bluebottle-09")))
    );
    // Nothing is highlighted until the user moves or types.
    press("down", cx);
    press("enter", cx);
    assert!(cx.update(|_, cx| view.read(cx).edit_error().is_some()));
    assert_eq!(cx.update(|_, cx| view.read(cx).open_menu()), None);
}

// The database ---------------------------------------------------------------

/// A scratch database: Current and Joint account, four rows on 9 Nov 2026.
struct Scratch {
    _dir: TempDir,
    path: std::path::PathBuf,
    current: AccountId,
    rows: Vec<TransactionId>,
}

impl Scratch {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("coffer.db");
        let mut db = Database::open(&path).unwrap();
        let mut create = |name: &str| {
            account::create(
                &mut db,
                &NewAccount::new(name, AccountKind::Current, Currency::GBP),
            )
            .unwrap()
        };
        let current = create("Current account");
        let joint = create("Joint account");
        let day = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let rows = [
            (current, -420, "SQ *BLUE BOTTLE COFFEE"),
            (current, -1899, "STEAMGAMES"),
            (joint, -1675, "VINTED *KX8F2Q"),
            (joint, -2399, "AMAZON MARKETPLACE"),
        ]
        .into_iter()
        .map(|(account, minor, text)| {
            manual::create(
                &mut db,
                &ManualTransaction::new(account, day, Money::new(minor, Currency::GBP), text),
            )
            .unwrap()
        })
        .collect();
        Self {
            _dir: dir,
            path,
            current,
            rows,
        }
    }

    /// The stored category key of `id`, read afresh.
    fn category(&self, id: TransactionId) -> Option<String> {
        let today = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
        let register = TransactionStore::new(self.path.clone())
            .load(today)
            .unwrap();
        match register.row(id).unwrap().category {
            RowCategory::Category(category) => {
                Some(register.category(category).unwrap().key.to_string())
            }
            _ => None,
        }
    }
}

fn open_database<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
) -> (Entity<TransactionsView>, &'a mut VisualTestContext) {
    setup(cx);
    let store = TransactionStore::new(scratch.path.clone());
    let (view, cx) = cx.add_window_view(move |window, cx| {
        TransactionsView::new(Source::Database(store), window, cx)
    });
    redraw(cx);
    (view, cx)
}

#[gpui_kit::test]
fn the_table_reads_the_database(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let mut newest_first = scratch.rows.clone();
    newest_first.reverse();
    assert_eq!(listed(&view, cx), newest_first);
    assert!(shows(row_id(scratch.rows[0]), cx));
    assert_eq!(cx.update(|_, cx| view.read(cx).listing().uncategorised), 4);
}

#[gpui_kit::test]
fn a_category_set_in_place_is_saved_through_core(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let coffee = scratch.rows[0];
    click(row_id(coffee), cx);
    press("enter", cx);
    assert_eq!(
        cx.update(|_, cx| view.read(cx).open_menu()),
        Some(Menu::Row(coffee))
    );

    // The filter field narrows the 67 categories; Enter takes the first.
    type_text("eating", cx);
    press("enter", cx);
    assert_eq!(cx.update(|_, cx| view.read(cx).open_menu()), None);
    assert_eq!(scratch.category(coffee).as_deref(), Some("food.eating_out"));

    // The register is read again, and the row keeps its selection.
    let category = cx.update(|_, cx| {
        let view = view.read(cx);
        match view.register().row(coffee).unwrap().category {
            RowCategory::Category(id) => view.register().category(id).unwrap().label.clone(),
            _ => String::new(),
        }
    });
    assert_eq!(category, "Eating out");
    assert_eq!(selected(&view, cx), [coffee]);
    assert!(
        cx.update(|_, cx| view.read(cx).undo_message().is_none()),
        "one row: no undo bar"
    );
}

#[gpui_kit::test]
fn a_bulk_categorise_can_be_undone(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let ids = scratch.rows[..3].to_vec();
    cx.update(|_, cx| view.update(cx, |view, cx| view.select(ids.clone(), cx)));
    redraw(cx);

    click("bulk-categorise", cx);
    assert_eq!(
        cx.update(|_, cx| view.read(cx).open_menu()),
        Some(Menu::Bulk)
    );
    type_text("clothing", cx);
    press("enter", cx);
    for id in &ids {
        assert_eq!(scratch.category(*id).as_deref(), Some("shopping.clothing"));
    }
    assert_eq!(
        cx.update(|_, cx| view.read(cx).undo_message().cloned()),
        Some("Categorised 3 transactions as Clothing".into())
    );
    assert!(shows("undo-change", cx));

    click("undo-change", cx);
    for id in &ids {
        assert_eq!(scratch.category(*id), None);
    }
    assert!(!shows("undo-change", cx));
}

#[gpui_kit::test]
fn the_category_chip_filters_by_a_group(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (view, cx) = open_database(cx, &scratch);
    let coffee = scratch.rows[0];
    {
        let mut db = Database::open(&scratch.path).unwrap();
        coffer_core::inbox::own::recategorize(
            &mut db,
            &[(
                coffee,
                coffer_core::category::CategoryKey::new("food.eating_out").unwrap(),
            )],
        )
        .unwrap();
    }
    cx.update(|_, cx| view.update(cx, |view, cx| view.reload(cx)));
    redraw(cx);

    click("filter-category", cx);
    type_text("food", cx);
    // "Food" heads its group: the first option.
    press("enter", cx);
    assert_eq!(
        listed(&view, cx),
        [coffee],
        "a group includes its subcategories"
    );
}

#[gpui_kit::test]
fn the_panels_account_filters_the_table(cx: &mut TestAppContext) {
    use crate::accounts::Accounts;
    use crate::accounts::store::AccountStore;
    use crate::shell::{ShellView, View};

    let scratch = Scratch::new();
    setup(cx);
    cx.update(crate::shell::init);
    let (accounts_store, store) = (
        AccountStore::new(scratch.path.clone()),
        TransactionStore::new(scratch.path.clone()),
    );
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        let accounts = cx.new(|_| Accounts::from_store(accounts_store));
        ShellView::with_sources(
            View::Transactions,
            accounts,
            Source::Database(store),
            window,
            cx,
        )
    });
    redraw(cx);
    let view = cx.update(|_, cx| shell.read(cx).transactions().clone());
    assert_eq!(listed(&view, cx).len(), 4);

    click(format!("account-{}", scratch.current), cx);
    let mut current: Vec<_> = scratch.rows[..2].to_vec();
    current.reverse();
    assert_eq!(listed(&view, cx), current);
    assert!(
        !shows("sort-account", cx),
        "the Account column hides on one account"
    );

    // The Account chip mirrors the panel: All accounts clears it.
    click("filter-account", cx);
    click("option-all", cx);
    assert_eq!(listed(&view, cx).len(), 4);
    let accounts = cx.update(|_, cx| shell.read(cx).accounts().clone());
    assert_eq!(cx.update(|_, cx| accounts.read(cx).selected()), None);
}
