//! The app shell (ROADMAP M3.1; `docs/DESIGN.md` → "Finance screens"): the
//! one window `coffer` opens, and the frame every view sits in.
//!
//! Top to bottom: the title bar, with the eight view tabs (Overview,
//! Transactions, Trends, Budgets, Net worth, Inbox, Reviews, Rules) and the
//! Settings button; the body, with the view's left panel and its canvas (and
//! inspector); the status bar. Navigation is the title bar's, as Paper draws
//! it (Rules `B1J-0`, Settings · AI clients `AQ2-0`), not a sidebar: the
//! left panel belongs to the view. The five views of the money share the
//! accounts panel ([`crate::accounts::panel`]); Inbox, Reviews, Rules and
//! Settings get their own lists with their screens.
//!
//! Keyboard: ⌘1–⌘8 open the views in tab order and ⌘, opens Settings
//! (Ctrl on Linux and Windows). Clicking the Settings button while Settings
//! is open goes back to the view it was opened from.
//!
//! Transactions is the only screen built so far (D5; on the database since
//! M3.3). The others show the shell around an [`EmptyState`] that says so,
//! until their roadmap items land.
//!
//! The shell owns which view is open and the window's saved state
//! ([`window_state`]): [`open`] restores the bounds and the view from the
//! database, and the shell saves them again shortly after they change and
//! when the window closes.
//!
//! It also owns the accounts ([`Accounts`], M3.2), which the panel, the
//! Transactions inspector and the account dialogs share, and opens those
//! dialogs over everything when the panel or inspector asks.

pub mod window_state;

use std::rc::Rc;
use std::time::Duration;

use coffer_core::paths;
use coffer_core::setting::WindowState;
use gpui_kit::AnyView;
use gpui_kit::component::window_border;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, App, AppContext as _, Bounds, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, KeyBinding, KeyBindingContextPredicate, ParentElement as _, Render, SharedString,
    Styled as _, Subscription, Task, Window, WindowOptions, div, px, size,
};

use crate::accounts::add_balance::AddBalanceDialog;
use crate::accounts::new_account::NewAccountDialog;
use crate::accounts::store::AccountStore;
use crate::accounts::{self, Accounts, AccountsEvent, DialogClosed};
use crate::theme::tokens::{color, size as token_size, typography};
use crate::transactions::store::TransactionStore;
use crate::transactions::{self, TransactionsView};
use crate::ui::button::IconButton;
use crate::ui::empty_state::EmptyState;
use crate::ui::icon::Glyph;
use crate::ui::status_bar::{StatusBar, StatusItem};
use crate::ui::text::TextRoleExt as _;
use crate::ui::title_bar::{self, CommandBar, TitleBar, ViewTab};

use window_state::WindowStore;

/// The key context of the whole window, where the view shortcuts work.
const SHELL_CONTEXT: &str = "CofferShell";

/// How long the window must stay put before its state is saved, so a drag
/// or a resize writes once, after it ends.
const SAVE_DELAY: Duration = Duration::from_millis(500);

gpui_kit::actions!(
    coffer_shell,
    [
        /// ⌘1: open Overview.
        ShowOverview,
        /// ⌘2: open Transactions.
        ShowTransactions,
        /// ⌘3: open Trends.
        ShowTrends,
        /// ⌘4: open Budgets.
        ShowBudgets,
        /// ⌘5: open Net worth.
        ShowNetWorth,
        /// ⌘6: open the Inbox.
        ShowInbox,
        /// ⌘7: open Reviews.
        ShowReviews,
        /// ⌘8: open Rules.
        ShowRules,
        /// ⌘,: open Settings.
        OpenSettings,
    ]
);

/// One of Coffer's views: a tab in the title bar, or Settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum View {
    Overview,
    /// The view a first launch opens: the only one built so far.
    #[default]
    Transactions,
    Trends,
    Budgets,
    NetWorth,
    Inbox,
    Reviews,
    Rules,
    /// Not a tab: the title bar's Settings button opens it, and while it's
    /// open no tab is active (`DESIGN.md` → "Settings").
    Settings,
}

impl View {
    /// The title bar's tabs, in order.
    pub const TABS: [View; 8] = [
        View::Overview,
        View::Transactions,
        View::Trends,
        View::Budgets,
        View::NetWorth,
        View::Inbox,
        View::Reviews,
        View::Rules,
    ];

    /// Every view: the tabs, then Settings.
    pub const ALL: [View; 9] = [
        View::Overview,
        View::Transactions,
        View::Trends,
        View::Budgets,
        View::NetWorth,
        View::Inbox,
        View::Reviews,
        View::Rules,
        View::Settings,
    ];

    /// The key the window state saves the view under. Stable: changing one
    /// reopens saved windows on the default view.
    pub fn key(self) -> &'static str {
        match self {
            View::Overview => "overview",
            View::Transactions => "transactions",
            View::Trends => "trends",
            View::Budgets => "budgets",
            View::NetWorth => "net_worth",
            View::Inbox => "inbox",
            View::Reviews => "reviews",
            View::Rules => "rules",
            View::Settings => "settings",
        }
    }

    /// The view saved under `key`, if this build has it.
    pub fn from_key(key: &str) -> Option<View> {
        View::ALL.into_iter().find(|view| view.key() == key)
    }

    /// The name on its tab and in its title.
    pub fn label(self) -> &'static str {
        match self {
            View::Overview => "Overview",
            View::Transactions => "Transactions",
            View::Trends => "Trends",
            View::Budgets => "Budgets",
            View::NetWorth => "Net worth",
            View::Inbox => "Inbox",
            View::Reviews => "Reviews",
            View::Rules => "Rules",
            View::Settings => "Settings",
        }
    }

    /// The element id of its tab, or of the Settings button.
    pub fn element_id(self) -> &'static str {
        match self {
            View::Overview => "view-overview",
            View::Transactions => "view-transactions",
            View::Trends => "view-trends",
            View::Budgets => "view-budgets",
            View::NetWorth => "view-net-worth",
            View::Inbox => "view-inbox",
            View::Reviews => "view-reviews",
            View::Rules => "view-rules",
            View::Settings => "open-settings",
        }
    }

    /// Whether the view filters by account, and so has the accounts panel
    /// on its left (`DESIGN.md` → "Accounts panel").
    pub fn has_accounts_panel(self) -> bool {
        matches!(
            self,
            View::Overview | View::Transactions | View::Trends | View::Budgets | View::NetWorth
        )
    }

    /// Whether its screen exists yet.
    pub fn is_built(self) -> bool {
        self == View::Transactions
    }
}

/// Binds the shell's shortcuts and the Transactions table's keys. Call once,
/// before [`open`] or the first [`ShellView`].
pub fn init(cx: &mut App) {
    let bindings: [(&str, Box<dyn Action>); 9] = [
        ("secondary-1", Box::new(ShowOverview)),
        ("secondary-2", Box::new(ShowTransactions)),
        ("secondary-3", Box::new(ShowTrends)),
        ("secondary-4", Box::new(ShowBudgets)),
        ("secondary-5", Box::new(ShowNetWorth)),
        ("secondary-6", Box::new(ShowInbox)),
        ("secondary-7", Box::new(ShowReviews)),
        ("secondary-8", Box::new(ShowRules)),
        ("secondary-,", Box::new(OpenSettings)),
    ];
    let context = Rc::new(
        KeyBindingContextPredicate::parse(SHELL_CONTEXT).expect("the shell's key context parses"),
    );
    // `KeyBinding::new` can't resolve `secondary-` (⌘ on macOS, Ctrl
    // elsewhere); `load` asks the platform.
    let bindings = bindings.map(|(keys, action)| {
        KeyBinding::load(keys, action, Some(context.clone()), false, None, cx)
            .expect("the shell's shortcuts parse")
    });
    cx.bind_keys(bindings);
    transactions::init(cx);
    accounts::init(cx);
}

/// Opens Coffer's window as it was last left: the saved bounds and view
/// from the database in the data dir, or 1440×900 (the size it's designed
/// at, where the screen allows) on Transactions.
pub fn open(cx: &mut App) -> gpui_kit::Result<()> {
    let store = match paths::database_path() {
        Ok(database) => Some(WindowStore::new(database)),
        Err(error) => {
            tracing::warn!(%error, "no data dir; the window state won't be kept");
            None
        }
    };
    let (accounts, source) = match paths::database_path() {
        Ok(database) => (
            Accounts::from_store(AccountStore::new(database.clone())),
            transactions::Source::Database(TransactionStore::new(database)),
        ),
        // Without a data folder there's nothing to read: the panel says why,
        // and the table shows the sample data.
        Err(error) => (
            Accounts::unavailable(format!("Coffer has no data folder: {error}")),
            transactions::Source::Fixture,
        ),
    };
    let saved = store.as_ref().and_then(WindowStore::load);
    let fallback = Bounds::centered(None, size(px(1440.), px(900.)), cx);
    let displays: Vec<_> = cx
        .displays()
        .iter()
        .map(|display| display.bounds())
        .collect();
    let options = WindowOptions {
        window_bounds: Some(window_state::restored_bounds(
            saved.as_ref(),
            &displays,
            fallback,
        )),
        window_min_size: Some(size(
            token_size::WINDOW_MIN_WIDTH,
            token_size::WINDOW_MIN_HEIGHT,
        )),
        ..title_bar::window_options()
    };
    let view = window_state::restored_view(saved.as_ref());
    gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| {
            let accounts = cx.new(|_| accounts);
            let mut shell = ShellView::with_sources(view, accounts, source, window, cx);
            if let Some(store) = store {
                shell.keep_window_state(store, window, cx);
            }
            shell
        })
    })?;
    Ok(())
}

/// Saving the window state: the latest state, the one last written, and the
/// pending delayed save.
struct Persistence {
    store: WindowStore,
    latest: WindowState,
    saved: Option<WindowState>,
    pending: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// The window's root view: the shell around the open view.
pub struct ShellView {
    view: View,
    /// The view to go back to when the Settings button is clicked again: the
    /// one open before Settings.
    before_settings: View,
    transactions: Entity<TransactionsView>,
    accounts: Entity<Accounts>,
    /// The dialog open over the window, and its subscription.
    dialog: Option<(AnyView, Subscription)>,
    /// The accounts panel's focus: a Tab stop where the arrow keys move the
    /// selected account.
    panel_focus: FocusHandle,
    focus: FocusHandle,
    persistence: Option<Persistence>,
    _subscriptions: Vec<Subscription>,
}

impl ShellView {
    /// The shell showing `view` with the sample accounts, keeping no window
    /// state (tests, snapshots).
    pub fn new(view: View, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let accounts = cx.new(|_| Accounts::fixture(accounts::fixture::accounts()));
        Self::with_accounts(view, accounts, window, cx)
    }

    /// The shell showing `view` with `accounts` in its panel and the
    /// sample transactions.
    pub fn with_accounts(
        view: View,
        accounts: Entity<Accounts>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_sources(view, accounts, transactions::Source::Fixture, window, cx)
    }

    /// The shell showing `view` with `accounts` in its panel and the
    /// transactions from `source`.
    pub fn with_sources(
        view: View,
        accounts: Entity<Accounts>,
        source: transactions::Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        // Focused from the start, so the shortcuts work before any click.
        focus.focus(window, cx);
        let transactions = cx.new(|cx| {
            let mut transactions = TransactionsView::new(source, window, cx);
            transactions.show_accounts(accounts.clone(), window, cx);
            transactions
        });
        let subscriptions = vec![
            cx.subscribe_in(&accounts, window, |this, _, event, window, cx| {
                this.on_accounts_event(*event, window, cx)
            }),
            cx.observe(&accounts, |_, _, cx| cx.notify()),
        ];
        Self {
            view,
            before_settings: View::default(),
            transactions,
            accounts,
            dialog: None,
            panel_focus: cx.focus_handle(),
            focus,
            persistence: None,
            _subscriptions: subscriptions,
        }
    }

    /// The accounts the panel shows.
    pub fn accounts(&self) -> &Entity<Accounts> {
        &self.accounts
    }

    /// The dialog open over the window, if any.
    pub fn dialog(&self) -> Option<&AnyView> {
        self.dialog.as_ref().map(|(view, _)| view)
    }

    fn on_accounts_event(
        &mut self,
        event: AccountsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            // Selecting an account shows it in the inspector, which a
            // selected transaction would cover.
            AccountsEvent::Selected(Some(_)) => self
                .transactions
                .update(cx, |transactions, cx| transactions.clear_selection(cx)),
            AccountsEvent::Selected(None) => self.transactions.update(cx, |_, cx| cx.notify()),
            AccountsEvent::NewAccountRequested => {
                let accounts = self.accounts.clone();
                let dialog = cx.new(|cx| NewAccountDialog::new(accounts, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            AccountsEvent::AddBalanceRequested(account) => {
                let accounts = self.accounts.clone();
                let dialog = cx.new(|cx| AddBalanceDialog::new(accounts, account, window, cx));
                self.open_dialog(dialog, window, cx);
            }
        }
    }

    /// Shows `dialog` over the window until it closes, then puts focus back
    /// on the shell.
    fn open_dialog<D>(&mut self, dialog: Entity<D>, window: &mut Window, cx: &mut Context<Self>)
    where
        D: Render + gpui_kit::EventEmitter<DialogClosed>,
    {
        let subscription =
            cx.subscribe_in(&dialog, window, |this, _, _: &DialogClosed, window, cx| {
                this.dialog = None;
                this.focus.focus(window, cx);
                cx.notify();
            });
        self.dialog = Some((dialog.into(), subscription));
        cx.notify();
    }

    /// The open view.
    pub fn view(&self) -> View {
        self.view
    }

    /// The Transactions screen, which stays alive while other views are
    /// open, so its selection and scroll position survive a switch.
    pub fn transactions(&self) -> &Entity<TransactionsView> {
        &self.transactions
    }

    /// Opens `view`, and takes focus back to the shell: whatever had it (the
    /// Transactions table) may have left the window, and a focus outside the
    /// window would leave the shortcuts nowhere to go.
    pub fn show(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        if self.view == view {
            return;
        }
        self.focus.focus(window, cx);
        if view == View::Settings {
            self.before_settings = self.view;
        }
        self.view = view;
        self.remember(window, cx);
        cx.notify();
    }

    /// The Settings button: opens Settings, or while it's open goes back to
    /// the view open before it (Transactions when the window opened on
    /// Settings). ⌘, only opens it.
    pub fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = if self.view == View::Settings {
            self.before_settings
        } else {
            View::Settings
        };
        self.show(view, window, cx);
    }

    /// Saves the window state to `store` from now on: shortly after the
    /// window moves, resizes or changes view, and when it closes or the app
    /// quits.
    pub fn keep_window_state(
        &mut self,
        store: WindowStore,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let subscriptions = vec![
            cx.observe_window_bounds(window, |this, window, cx| this.remember(window, cx)),
            cx.on_release(|this, _| this.save_now()),
            cx.on_app_quit(|this, _| {
                this.save_now();
                async {}
            }),
        ];
        self.persistence = Some(Persistence {
            store,
            latest: window_state::snapshot(window.window_bounds(), self.view),
            saved: None,
            pending: None,
            _subscriptions: subscriptions,
        });
    }

    /// Notes the window's current state and saves it after [`SAVE_DELAY`],
    /// unless it changes again first.
    fn remember(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(persistence) = self.persistence.as_mut() else {
            return;
        };
        let state = window_state::snapshot(window.window_bounds(), self.view);
        if state == persistence.latest {
            return;
        }
        persistence.latest = state;
        let store = persistence.store.clone();
        // Replacing the task cancels the save it was waiting to make.
        persistence.pending = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let Ok(Some(state)) = this.update(cx, |this, _| this.take_unsaved()) else {
                return;
            };
            cx.background_executor()
                .spawn(async move { store.save(&state) })
                .await;
        }));
    }

    /// The latest state if it hasn't been saved, marked as saved.
    fn take_unsaved(&mut self) -> Option<WindowState> {
        let persistence = self.persistence.as_mut()?;
        if persistence.saved.as_ref() == Some(&persistence.latest) {
            return None;
        }
        persistence.saved = Some(persistence.latest.clone());
        persistence.saved.clone()
    }

    /// Saves the latest state now, on this thread: the window is closing or
    /// the app is quitting, and a background save might not finish.
    fn save_now(&mut self) {
        if let Some(persistence) = self.persistence.as_mut() {
            persistence.pending = None;
        }
        if let Some(state) = self.take_unsaved()
            && let Some(persistence) = &self.persistence
        {
            persistence.store.save(&state);
        }
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> TitleBar {
        let bar = View::TABS.into_iter().fold(
            TitleBar::new(CommandBar::new("Search transactions or run a command")),
            |bar, view| {
                let mut tab = ViewTab::new(view.element_id(), view.label())
                    .active(self.view == view)
                    .on_click(cx.listener(move |this, _, window, cx| this.show(view, window, cx)));
                if view == View::Inbox {
                    // Fixture: the pending suggestions on the Paper
                    // artboards, until the Inbox reads them (M3.6).
                    tab = tab.badge("12");
                }
                bar.tab(tab)
            },
        );
        bar.settings(
            IconButton::new(
                View::Settings.element_id(),
                Glyph::Settings,
                View::Settings.label(),
            )
            .active(self.view == View::Settings)
            .on_click(cx.listener(|this, _, window, cx| this.toggle_settings(window, cx))),
        )
    }

    /// The canvas of a view whose screen isn't built yet: the empty state,
    /// centred (`DESIGN.md` → "Review queues": an empty canvas centres its
    /// message).
    fn render_unbuilt(&self) -> impl IntoElement {
        let view = self.view;
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .justify_center()
            .bg(color::CANVAS)
            .child(
                EmptyState::new(format!("{} isn't built yet", view.label()))
                    .body("It comes in a later version of Coffer. Transactions works now.")
                    .shortcut("⌘2", "Open Transactions"),
            )
    }
}

/// The status bar, with the Transactions fixture's values on every view
/// until each reads the database.
fn status_bar() -> StatusBar {
    // The fixture holds a page of the month; the status bar counts the
    // whole import, as Paper does.
    const IMPORTED: usize = 328;
    StatusBar::new()
        .start(StatusItem::text("1 agent connected").agent_dot())
        .start(StatusItem::text("Reconciled").icon(Glyph::Check))
        .start(StatusItem::text(
            "Imported 2 min ago from monzo-november.csv",
        ))
        .start(StatusItem::mono(SharedString::from(format!(
            "{IMPORTED} transactions"
        ))))
        .end(StatusItem::text("3 new").new_dot())
        .end(StatusItem::mono("GBP"))
}

impl Render for ShellView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.view;
        let body = div()
            .flex()
            .flex_1()
            .min_h_0()
            .when(view.has_accounts_panel(), |this| {
                this.child(accounts::panel::render(
                    &self.accounts,
                    &self.panel_focus,
                    window,
                    cx,
                ))
            })
            .map(|this| {
                if view == View::Transactions {
                    this.child(self.transactions.clone())
                } else {
                    this.child(self.render_unbuilt())
                }
            });
        window_border().child(
            div()
                .key_context(SHELL_CONTEXT)
                .track_focus(&self.focus)
                .on_action(cx.listener(|this, _: &ShowOverview, window, cx| {
                    this.show(View::Overview, window, cx)
                }))
                .on_action(cx.listener(|this, _: &ShowTransactions, window, cx| {
                    this.show(View::Transactions, window, cx)
                }))
                .on_action(cx.listener(|this, _: &ShowTrends, window, cx| {
                    this.show(View::Trends, window, cx)
                }))
                .on_action(cx.listener(|this, _: &ShowBudgets, window, cx| {
                    this.show(View::Budgets, window, cx)
                }))
                .on_action(cx.listener(|this, _: &ShowNetWorth, window, cx| {
                    this.show(View::NetWorth, window, cx)
                }))
                .on_action(
                    cx.listener(|this, _: &ShowInbox, window, cx| {
                        this.show(View::Inbox, window, cx)
                    }),
                )
                .on_action(cx.listener(|this, _: &ShowReviews, window, cx| {
                    this.show(View::Reviews, window, cx)
                }))
                .on_action(
                    cx.listener(|this, _: &ShowRules, window, cx| {
                        this.show(View::Rules, window, cx)
                    }),
                )
                .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                    this.show(View::Settings, window, cx)
                }))
                .size_full()
                .flex()
                .flex_col()
                .bg(color::CHROME)
                .text_role(typography::BODY)
                .text_color(color::TEXT)
                .child(self.render_title_bar(cx))
                .child(body)
                .child(status_bar())
                .when_some(
                    self.dialog.as_ref().map(|(view, _)| view.clone()),
                    |this, dialog| this.child(dialog),
                ),
        )
    }
}

#[cfg(test)]
mod tests;
