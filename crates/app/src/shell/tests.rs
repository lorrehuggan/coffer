use coffer_core::setting::WindowMode;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{TestAppContext, VisualTestContext};
use tempfile::TempDir;

use super::*;

/// A transaction row on the Transactions screen, present only while it
/// shows.
/// The fixture's Amazon payment, the row the window opens on.
fn a_row() -> String {
    format!("txn-{}", crate::transactions::fixture::id("txn-amazon-09"))
}

/// The accounts panel's "Add account" button, present only while the panel
/// shows.
const ACCOUNTS_PANEL: &str = "add-account";

fn open_shell(cx: &mut TestAppContext, view: View) -> (Entity<ShellView>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        init(cx);
    });
    let (shell, cx) = cx.add_window_view(|window, cx| ShellView::new(view, window, cx));
    redraw(cx);
    (shell, cx)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn view(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> View {
    cx.update(|_, cx| shell.read(cx).view())
}

fn shows(id: &str, cx: &mut VisualTestContext) -> bool {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, _| window.try_find(id).is_some())
}

/// A shortcut with the platform's command key: ⌘ on macOS, Ctrl elsewhere,
/// as `secondary-` binds it.
fn secondary(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{key}")
    } else {
        format!("ctrl-{key}")
    }
}

#[test]
fn the_tabs_are_papers_eight_views_in_order() {
    // Paper `B8Y-0` (Rules), the first artboard with the eighth tab.
    let labels: Vec<_> = View::TABS.into_iter().map(View::label).collect();
    assert_eq!(
        labels,
        [
            "Overview",
            "Transactions",
            "Trends",
            "Budgets",
            "Net worth",
            "Inbox",
            "Reviews",
            "Rules"
        ]
    );
    assert!(
        !View::TABS.contains(&View::Settings),
        "Settings isn't a tab"
    );
}

#[test]
fn every_view_round_trips_through_its_key() {
    for view in View::ALL {
        assert_eq!(View::from_key(view.key()), Some(view), "{view:?}");
        // Keys must pass core's check on the saved state.
        let state =
            window_state::snapshot(gpui_kit::WindowBounds::Windowed(Bounds::default()), view);
        assert_eq!(state.view.as_deref(), Some(view.key()));
        assert!(
            view.key()
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        );
    }
    assert_eq!(View::from_key("forecast"), None);
}

#[test]
fn the_money_views_share_the_accounts_panel() {
    let with_panel: Vec<_> = View::ALL
        .into_iter()
        .filter(|view| view.has_accounts_panel())
        .collect();
    assert_eq!(
        with_panel,
        [
            View::Overview,
            View::Transactions,
            View::Trends,
            View::Budgets,
            View::NetWorth
        ]
    );
}

#[gpui_kit::test]
fn clicking_a_tab_opens_its_view(cx: &mut TestAppContext) {
    let (shell, cx) = open_shell(cx, View::Transactions);
    assert!(shows(&a_row(), cx), "Transactions shows its table");
    assert!(shows(ACCOUNTS_PANEL, cx));

    cx.update(|window, cx| window.click("view-rules", cx));
    assert_eq!(view(&shell, cx), View::Rules);
    redraw(cx);
    assert!(!shows(&a_row(), cx), "Rules isn't the table");
    assert!(!shows(ACCOUNTS_PANEL, cx), "Rules has no accounts panel");

    cx.update(|window, cx| window.click("view-budgets", cx));
    assert_eq!(view(&shell, cx), View::Budgets);
    redraw(cx);
    assert!(shows(ACCOUNTS_PANEL, cx), "Budgets filters by account");

    cx.update(|window, cx| window.click("view-transactions", cx));
    redraw(cx);
    assert!(shows(&a_row(), cx));
}

/// The tabs that report themselves selected to assistive technology.
fn selected_tabs(cx: &mut VisualTestContext) -> Vec<View> {
    cx.update(|window, _| {
        View::TABS
            .into_iter()
            .filter(|view| {
                let tab = window.find(view.element_id());
                assert_eq!(tab.role(), Some(gpui_kit::Role::Tab), "{view:?}");
                tab.selected() == Some(true)
            })
            .collect()
    })
}

#[gpui_kit::test]
fn the_open_view_is_the_selected_tab(cx: &mut TestAppContext) {
    let (_shell, cx) = open_shell(cx, View::Transactions);
    assert_eq!(selected_tabs(cx), [View::Transactions]);
    cx.update(|window, cx| window.click("view-reviews", cx));
    redraw(cx);
    assert_eq!(selected_tabs(cx), [View::Reviews]);
}

#[gpui_kit::test]
fn the_settings_button_opens_settings(cx: &mut TestAppContext) {
    let (shell, cx) = open_shell(cx, View::Inbox);
    cx.update(|window, cx| window.click("open-settings", cx));
    assert_eq!(view(&shell, cx), View::Settings);
    redraw(cx);
    assert!(!shows(ACCOUNTS_PANEL, cx));
    assert!(selected_tabs(cx).is_empty(), "no tab is active in Settings");
}

#[gpui_kit::test]
fn clicking_settings_again_goes_back(cx: &mut TestAppContext) {
    let (shell, cx) = open_shell(cx, View::Budgets);
    cx.update(|window, cx| window.click("open-settings", cx));
    assert_eq!(view(&shell, cx), View::Settings);
    redraw(cx);
    cx.update(|window, cx| window.click("open-settings", cx));
    assert_eq!(
        view(&shell, cx),
        View::Budgets,
        "back to where it was opened"
    );
    redraw(cx);
    assert!(shows(ACCOUNTS_PANEL, cx));

    // ⌘, only opens Settings; it never closes it.
    cx.simulate_keystrokes(&secondary(","));
    cx.simulate_keystrokes(&secondary(","));
    assert_eq!(view(&shell, cx), View::Settings);
}

#[gpui_kit::test]
fn a_window_opened_on_settings_goes_back_to_the_default(cx: &mut TestAppContext) {
    let (shell, cx) = open_shell(cx, View::Settings);
    cx.update(|window, cx| window.click("open-settings", cx));
    assert_eq!(view(&shell, cx), View::default());
}

#[gpui_kit::test]
fn shortcuts_open_the_views(cx: &mut TestAppContext) {
    let (shell, cx) = open_shell(cx, View::Transactions);
    for (key, expected) in View::TABS.into_iter().enumerate() {
        cx.simulate_keystrokes(&secondary(&(key + 1).to_string()));
        assert_eq!(
            view(&shell, cx),
            expected,
            "{}",
            secondary(&(key + 1).to_string())
        );
    }
    cx.simulate_keystrokes(&secondary(","));
    assert_eq!(view(&shell, cx), View::Settings);
}

#[gpui_kit::test]
fn shortcuts_work_from_inside_the_table(cx: &mut TestAppContext) {
    let (shell, cx) = open_shell(cx, View::Transactions);
    // A click focuses the table; its keys still reach the shell.
    cx.update(|window, cx| window.click(a_row(), cx));
    cx.simulate_keystrokes(&secondary("8"));
    assert_eq!(view(&shell, cx), View::Rules);
    // The focused table has left the window; the next shortcut still lands.
    redraw(cx);
    cx.simulate_keystrokes(&secondary("2"));
    assert_eq!(view(&shell, cx), View::Transactions);
}

#[gpui_kit::test]
fn the_transactions_selection_survives_a_switch(cx: &mut TestAppContext) {
    let (shell, cx) = open_shell(cx, View::Transactions);
    let pret = crate::transactions::fixture::id("txn-pret-09");
    cx.update(|window, cx| window.click(format!("txn-{pret}"), cx));
    cx.update(|window, cx| window.click("view-overview", cx));
    redraw(cx);
    cx.update(|window, cx| window.click("view-transactions", cx));
    let selected = cx.update(|_, cx| {
        let transactions = shell.read(cx).transactions().read(cx);
        transactions
            .selected()
            .iter()
            .map(|transaction| transaction.id)
            .collect::<Vec<_>>()
    });
    assert_eq!(selected, [pret]);
}

/// A shell that saves its window state to a database in a scratch dir.
fn open_kept(
    cx: &mut TestAppContext,
) -> (
    TempDir,
    WindowStore,
    Entity<ShellView>,
    &mut VisualTestContext,
) {
    let dir = TempDir::new().unwrap();
    let store = WindowStore::new(dir.path().join("coffer.db"));
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        init(cx);
    });
    let kept = store.clone();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        let mut shell = ShellView::new(View::Transactions, window, cx);
        shell.keep_window_state(kept, window, cx);
        shell
    });
    redraw(cx);
    (dir, store, shell, cx)
}

#[gpui_kit::test]
fn a_view_change_is_saved_after_a_pause(cx: &mut TestAppContext) {
    let (_dir, store, shell, cx) = open_kept(cx);
    cx.update(|window, cx| window.click("view-net-worth", cx));
    assert_eq!(view(&shell, cx), View::NetWorth);

    cx.run_until_parked();
    assert_eq!(store.load(), None, "nothing is written straight away");

    cx.executor().advance_clock(SAVE_DELAY);
    cx.run_until_parked();
    let saved = store.load().expect("saved after the delay");
    assert_eq!(saved.view.as_deref(), Some("net_worth"));
    assert_eq!(saved.mode, WindowMode::Windowed);
}

#[gpui_kit::test]
fn changes_in_quick_succession_save_once_with_the_last(cx: &mut TestAppContext) {
    let (_dir, store, _shell, cx) = open_kept(cx);
    cx.update(|window, cx| window.click("view-trends", cx));
    cx.executor().advance_clock(SAVE_DELAY / 2);
    cx.update(|window, cx| window.click("view-reviews", cx));
    cx.executor().advance_clock(SAVE_DELAY / 2);
    cx.run_until_parked();
    assert_eq!(store.load(), None, "the first save was put off");

    cx.executor().advance_clock(SAVE_DELAY);
    cx.run_until_parked();
    assert_eq!(store.load().unwrap().view.as_deref(), Some("reviews"));
}

#[gpui_kit::test]
fn closing_the_window_saves_at_once(cx: &mut TestAppContext) {
    let (_dir, store, shell, cx) = open_kept(cx);
    cx.update(|window, cx| window.click("view-inbox", cx));
    // The window holds the last handle, as in the app.
    drop(shell);
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert_eq!(store.load().unwrap().view.as_deref(), Some("inbox"));
}
