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
//! Transactions (D5; on the database since M3.3), the Inbox (M3.6) and
//! Settings (M3.15, M3.16), both from the database, are built. The others
//! show the shell around an [`EmptyState`] that says so, until their roadmap
//! items land.
//!
//! The shell owns which view is open and the window's saved state
//! ([`window_state`]): [`open`] restores the bounds and the view from the
//! database, and the shell saves them again shortly after they change and
//! when the window closes.
//!
//! It also owns the accounts ([`Accounts`], M3.2), which the panel, the
//! Transactions inspector and the account dialogs share, and opens those
//! dialogs over everything when the panel or inspector asks; and Settings
//! ([`SettingsView`], M3.15, M3.16), whose dialogs it opens the same way,
//! and whose kill switch the status bar shows.

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
use crate::checks::lock::{LockDialog, UnlockDialog};
use crate::checks::store::ChecksStore;
use crate::checks::view::ChecksView;
use crate::checks::{self, Checks, ChecksEvent, Queue};
use crate::inbox::store::InboxStore;
use crate::inbox::view::InboxView;
use crate::inbox::{self, Inbox};
use crate::settings::enable::EnableDialog;
use crate::settings::export::ExportDialog;
use crate::settings::remove::RemoveDialog;
use crate::settings::restore::RestoreDialog;
use crate::settings::store::SettingsStore;
use crate::settings::{self, Section, SettingsEvent, SettingsView};
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
        matches!(self, View::Transactions | View::Inbox | View::Settings)
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
    inbox::init(cx);
    checks::init(cx);
    settings::init(cx);
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
    let inbox = match paths::database_path() {
        Ok(database) => Inbox::from_store(InboxStore::new(database)),
        Err(error) => Inbox::unavailable(format!("Coffer has no data folder: {error}")),
    };
    let checks = match paths::database_path() {
        Ok(database) => Checks::from_store(ChecksStore::new(database)),
        Err(error) => Checks::unavailable(format!("Coffer has no data folder: {error}")),
    };
    let settings_store = paths::database_path().map(SettingsStore::new);
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
            let inbox = cx.new(|_| inbox);
            let settings = cx.new(|cx| match settings_store {
                Ok(store) => SettingsView::from_store(store, cx),
                Err(error) => {
                    SettingsView::unavailable(format!("Coffer has no data folder: {error}"), cx)
                }
            });
            let mut shell =
                ShellView::with_all(view, accounts, source, inbox, settings, window, cx);
            let checks = cx.new(|_| checks);
            shell.install_checks(checks, window, cx);
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
    /// The suggestions (M3.6), which the Inbox's panel, canvas and the
    /// tab's badge share.
    inbox: Entity<Inbox>,
    inbox_view: Entity<InboxView>,
    /// The review queues (M3.12), under CHECKS in the Inbox's panel, and
    /// the status bar's failing balance checks.
    checks: Entity<Checks>,
    checks_view: Entity<ChecksView>,
    checks_subscriptions: Vec<Subscription>,
    settings: Entity<SettingsView>,
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

    /// The shell showing `view` with `accounts` in its panel, the sample
    /// transactions, an empty Inbox and Settings on sample data.
    pub fn with_accounts(
        view: View,
        accounts: Entity<Accounts>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_sources(view, accounts, transactions::Source::Fixture, window, cx)
    }

    /// The shell showing `view` with `accounts` in its panel, the
    /// transactions from `source`, an empty Inbox and Settings on sample
    /// data.
    pub fn with_sources(
        view: View,
        accounts: Entity<Accounts>,
        source: transactions::Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let inbox = cx.new(|_| Inbox::empty());
        let settings = cx.new(SettingsView::fixture);
        Self::with_all(view, accounts, source, inbox, settings, window, cx)
    }

    /// The shell showing `view` with `accounts`, the sample transactions,
    /// `inbox` and Settings on sample data.
    pub fn with_data(
        view: View,
        accounts: Entity<Accounts>,
        inbox: Entity<Inbox>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = cx.new(SettingsView::fixture);
        let source = transactions::Source::Fixture;
        Self::with_all(view, accounts, source, inbox, settings, window, cx)
    }

    /// The shell showing `view` with `accounts`, the sample transactions, an
    /// empty Inbox and `settings` as Settings.
    pub fn with_settings(
        view: View,
        accounts: Entity<Accounts>,
        settings: Entity<SettingsView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let inbox = cx.new(|_| Inbox::empty());
        let source = transactions::Source::Fixture;
        Self::with_all(view, accounts, source, inbox, settings, window, cx)
    }

    /// The shell showing `view` with `accounts`, the transactions from
    /// `source`, `inbox` and `settings`.
    pub fn with_all(
        view: View,
        accounts: Entity<Accounts>,
        source: transactions::Source,
        inbox: Entity<Inbox>,
        settings: Entity<SettingsView>,
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
            cx.observe(&inbox, |_, _, cx| cx.notify()),
            cx.subscribe_in(&settings, window, |this, _, event, window, cx| {
                this.on_settings_event(event.clone(), window, cx)
            }),
        ];
        let inbox_view = cx.new(|cx| InboxView::new(inbox.clone(), cx));
        if view == View::Inbox {
            inbox_view.read(cx).focus_handle().clone().focus(window, cx);
        }
        let checks = cx.new(|_| Checks::empty());
        let checks_view = cx.new(|cx| ChecksView::new(checks.clone(), cx));
        Self {
            view,
            before_settings: View::default(),
            transactions,
            accounts,
            inbox,
            inbox_view,
            checks,
            checks_view,
            checks_subscriptions: Vec::new(),
            settings,
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

    /// The suggestions the Inbox shows.
    pub fn inbox(&self) -> &Entity<Inbox> {
        &self.inbox
    }

    /// Uses `checks` for the review queues (the shell starts with empty
    /// ones that read nothing).
    pub fn install_checks(
        &mut self,
        checks: Entity<Checks>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.checks_subscriptions = vec![
            cx.subscribe_in(&checks, window, |this, _, event, window, cx| {
                this.on_checks_event(*event, window, cx)
            }),
            cx.observe(&checks, |_, _, cx| cx.notify()),
        ];
        self.checks_view = cx.new(|cx| ChecksView::new(checks.clone(), cx));
        self.checks = checks;
        cx.notify();
    }

    /// The review queues.
    pub fn checks(&self) -> &Entity<Checks> {
        &self.checks
    }

    fn on_checks_event(&mut self, event: ChecksEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            ChecksEvent::Shown(queue) => {
                if self.view != View::Inbox {
                    self.show(View::Inbox, window, cx);
                }
                let focus = match queue {
                    Some(_) => self.checks_view.read(cx).focus_handle().clone(),
                    None => self.inbox_view.read(cx).focus_handle().clone(),
                };
                focus.focus(window, cx);
                cx.notify();
            }
            ChecksEvent::LockRequested(account) => {
                let checks = self.checks.clone();
                let dialog = cx.new(|cx| LockDialog::new(checks, account, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            ChecksEvent::UnlockRequested(account) => {
                let checks = self.checks.clone();
                let dialog = cx.new(|cx| UnlockDialog::new(checks, account, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            ChecksEvent::AddBalanceRequested(account) => {
                self.on_accounts_event(AccountsEvent::AddBalanceRequested(account), window, cx)
            }
            // A merge, an undo or a lock changed what the accounts panel
            // and Transactions read.
            ChecksEvent::Changed => {
                self.accounts.update(cx, |accounts, cx| accounts.reload(cx));
                self.transactions
                    .update(cx, |transactions, cx| transactions.reload(cx));
            }
        }
    }

    /// Opens Balance checks on `account`'s failing check: the flagged dot
    /// in the Accounts panel, and the status bar's failing checks.
    pub fn show_balance_checks(
        &mut self,
        account: Option<coffer_core::id::AccountId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show(View::Inbox, window, cx);
        self.checks.update(cx, |checks, cx| match account {
            Some(account) => checks.show_account(account, cx),
            None => checks.show(Queue::Balances, cx),
        });
    }

    /// Settings: AI access, backups and export.
    pub fn settings(&self) -> &Entity<SettingsView> {
        &self.settings
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
            AccountsEvent::ChecksRequested(account) => {
                self.show_balance_checks(Some(account), window, cx)
            }
        }
    }

    fn on_settings_event(
        &mut self,
        event: SettingsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings = self.settings.clone();
        match event {
            SettingsEvent::EnableRequested(name) => {
                let dialog = cx.new(|cx| EnableDialog::new(settings, name, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            SettingsEvent::RemoveRequested(name) => {
                let dialog = cx.new(|cx| RemoveDialog::new(settings, name, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            SettingsEvent::RestoreRequested(target) => {
                let dialog = cx.new(|cx| RestoreDialog::new(settings, target, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            SettingsEvent::ExportRequested => {
                let dialog = cx.new(|cx| ExportDialog::new(settings, window, cx));
                self.open_dialog(dialog, window, cx);
            }
            // The database was replaced: everything reads it again.
            SettingsEvent::Restored => self.accounts.update(cx, |accounts, cx| accounts.reload(cx)),
            SettingsEvent::AiAccessChanged => cx.notify(),
        }
    }

    /// Opens Settings on AI clients: the status bar's AI item.
    pub fn show_ai_clients(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show(View::Settings, window, cx);
        self.settings
            .update(cx, |settings, cx| settings.show(Section::Clients, cx));
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
                // A balance added or a lock changed: the checks read again.
                this.checks.update(cx, |checks, cx| checks.reload(cx));
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
        if view == View::Inbox {
            // What approving would change is computed when shown (decision
            // record "Inbox impact preview", point 6), and A, R and ↑↓ work
            // at once.
            self.inbox.update(cx, |inbox, cx| inbox.reload(cx));
            // The CHECKS counts, and the open queue, are read again too.
            self.checks.update(cx, |checks, cx| checks.reload(cx));
            let focus = if self.checks.read(cx).queue().is_some() {
                self.checks_view.read(cx).focus_handle().clone()
            } else {
                self.inbox_view.read(cx).focus_handle().clone()
            };
            focus.focus(window, cx);
        } else {
            self.focus.focus(window, cx);
        }
        if view == View::Settings {
            self.before_settings = self.view;
            // What clients did and which backups exist change while it's
            // closed.
            self.settings.update(cx, |settings, cx| settings.reload(cx));
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
                    // Pending suggestions from AI clients; Coffer's own
                    // don't count (`DESIGN.md` → "Inbox impact preview").
                    let pending = self.inbox.read(cx).badge();
                    if pending > 0 {
                        tab = tab.badge(SharedString::from(pending.to_string()));
                    }
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
/// until each reads the database. The first item follows the kill switch
/// (M3.15): "AI access off" with the `refused` icon while it's off
/// (Paper `GW6-0`); either opens Settings → AI clients.
fn status_bar(ai_on: bool, failing: usize, cx: &mut Context<ShellView>) -> StatusBar {
    // The fixture holds a page of the month; the status bar counts the
    // whole import, as Paper does.
    const IMPORTED: usize = 328;
    let shell = cx.entity().downgrade();
    let open = move |window: &mut Window, cx: &mut gpui_kit::App| {
        let _ = shell.update(cx, |shell, cx| shell.show_ai_clients(window, cx));
    };
    let ai = if ai_on {
        StatusItem::text("1 agent connected").agent_dot()
    } else {
        StatusItem::text("AI access off").muted_icon(Glyph::Refused)
    };
    // A failing balance check replaces "Reconciled" (`DESIGN.md` → "Review
    // queues"), and opens Balance checks.
    let checks = if failing > 0 {
        let shell = cx.entity().downgrade();
        StatusItem::text(format!(
            "{failing} balance {} failing",
            if failing == 1 { "check" } else { "checks" }
        ))
        .danger_dot()
        .on_click("status-balance-checks", move |window, cx| {
            let _ = shell.update(cx, |shell, cx| shell.show_balance_checks(None, window, cx));
        })
    } else {
        StatusItem::text("Reconciled").icon(Glyph::Check)
    };
    StatusBar::new()
        .start(ai.on_click("status-ai-access", open))
        .start(checks)
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
            .when(view == View::Inbox, |this| {
                this.child(inbox::panel::render(&self.inbox, &self.checks, cx))
            })
            .map(|this| match view {
                View::Transactions => this.child(self.transactions.clone()),
                View::Inbox if self.checks.read(cx).queue().is_some() => {
                    this.child(self.checks_view.clone())
                }
                View::Inbox => this.child(self.inbox_view.clone()),
                View::Settings => this.child(self.settings.clone()),
                _ => this.child(self.render_unbuilt()),
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
                .child(status_bar(
                    self.settings.read(cx).ai_on(),
                    self.checks.read(cx).failing(),
                    cx,
                ))
                .when_some(
                    self.dialog.as_ref().map(|(view, _)| view.clone()),
                    |this, dialog| this.child(dialog),
                ),
        )
    }
}

#[cfg(test)]
mod tests;
