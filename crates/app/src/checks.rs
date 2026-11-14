//! The review queues (ROADMAP M3.12; `docs/DESIGN.md` → "Review queues";
//! `docs/ARCHITECTURE.md` → "Decision record: review queues" and "Decision
//! record: review queues (built)"): Coffer's own checks, under CHECKS in
//! the Inbox's left panel.
//!
//! - **Possible duplicates**: each pending pair, the selected one open in
//!   place side by side, with why it was flagged and what merging would do.
//!   Merge excludes one row as a duplicate (never deletes it) as an
//!   undoable change batch; "Not duplicates" dismisses the pair for good,
//!   undoable from the bar right after.
//! - **Balance checks**: every account's balances checked against its
//!   transactions, a failure's window and the pair that explains it, and
//!   locking an account's checked history (and unlocking it).
//!
//! [`Checks`] holds what the queues show (from [`store::ChecksStore`],
//! which opens the database per call), which queue is open, the
//! selection and the last outcome. Every change is core's
//! ([`coffer_core::duplicate`], [`coffer_core::reconciliation`],
//! [`coffer_core::change`]); this only picks the IDs. Merchant names and
//! bank text are untrusted and shown plain ([`text`]).

pub mod balances;
pub mod duplicates;
pub mod fixture;
pub mod lock;
pub mod store;
pub mod text;
pub mod view;

use coffer_core::checks::{AccountReview, CheckRow, Pair};
use coffer_core::id::{AccountId, ChangeBatchId, DuplicateCandidateId, TransactionId};
use coffer_core::reconciliation::BalanceRef;
use gpui_kit::{Context, EventEmitter, SharedString};

use store::{ChecksData, ChecksStore};

/// Binds the queues' keys: M merge, N not duplicates, ←→ keep the other
/// row, ↑↓ move, Escape clears the selection. Call once, before the first
/// [`view::ChecksView`].
pub fn init(cx: &mut gpui_kit::App) {
    use gpui_kit::KeyBinding;
    use gpui_kit::base::actions::{Cancel, SelectDown, SelectUp};
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, Some(view::CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(view::CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(view::CONTEXT)),
        KeyBinding::new("m", view::MergeSelected, Some(view::CONTEXT)),
        KeyBinding::new("n", view::DismissSelected, Some(view::CONTEXT)),
        KeyBinding::new("left", view::KeepOther, Some(view::CONTEXT)),
        KeyBinding::new("right", view::KeepOther, Some(view::CONTEXT)),
    ]);
}

/// The two queues under CHECKS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Queue {
    Duplicates,
    Balances,
}

/// What the queues ask their owner (the shell) to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChecksEvent {
    /// A queue was opened (`Some`) or closed for the suggestions (`None`):
    /// the shell shows and focuses the right canvas.
    Shown(Option<Queue>),
    /// "Lock through …": open the lock dialog.
    LockRequested(AccountId),
    /// "Unlock…": open the unlock dialog.
    UnlockRequested(AccountId),
    /// "Add balance…": the accounts' Add balance dialog.
    AddBalanceRequested(AccountId),
    /// Transactions or locks changed: what reads them should read again.
    Changed,
}

/// What a change can be undone with, from the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Undo {
    /// A merge's `duplicate_merge` batch, and the pair it merged.
    Merge(ChangeBatchId, DuplicateCandidateId),
    /// A dismissed pair, reopened.
    Dismiss(DuplicateCandidateId),
}

/// What the last change did, shown until the next one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It went through: the bar's message, and its undo.
    Done {
        message: SharedString,
        undo: Option<Undo>,
    },
    /// Core refused it; nothing changed.
    Refused(SharedString),
}

/// Where the queues are read from.
enum Source {
    Database(ChecksStore),
    /// Nothing to read (no data folder, or a test without a database).
    None,
}

/// The review queues' state (module docs).
pub struct Checks {
    source: Source,
    data: ChecksData,
    queue: Option<Queue>,
    pair: Option<DuplicateCandidateId>,
    /// The selected pair planned keeping its other row ("Keep this
    /// instead", ←→).
    swapped: Option<Pair>,
    check: Option<(AccountId, BalanceRef)>,
    outcome: Option<Outcome>,
    load_error: Option<SharedString>,
}

impl EventEmitter<ChecksEvent> for Checks {}

impl Checks {
    /// The queues in `store`, read now.
    pub fn from_store(store: ChecksStore) -> Self {
        let mut checks = Self::with_source(Source::Database(store));
        checks.load();
        checks
    }

    /// No queues, because they can't be read: `reason` says why.
    pub fn unavailable(reason: impl Into<SharedString>) -> Self {
        Self {
            load_error: Some(reason.into()),
            ..Self::with_source(Source::None)
        }
    }

    /// Empty queues that read nothing (tests).
    pub fn empty() -> Self {
        Self::with_source(Source::None)
    }

    fn with_source(source: Source) -> Self {
        Self {
            source,
            data: ChecksData::default(),
            queue: None,
            pair: None,
            swapped: None,
            check: None,
            outcome: None,
            load_error: None,
        }
    }

    pub fn data(&self) -> &ChecksData {
        &self.data
    }

    pub fn today(&self) -> chrono::NaiveDate {
        self.data
            .today
            .unwrap_or_else(crate::accounts::fixture::today)
    }

    /// The open queue; `None` while the Inbox shows suggestions.
    pub fn queue(&self) -> Option<Queue> {
        self.queue
    }

    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    pub fn load_error(&self) -> Option<&SharedString> {
        self.load_error.as_ref()
    }

    /// Pending possible duplicates: the panel's count.
    pub fn pending_pairs(&self) -> usize {
        self.data.review.pairs.len()
    }

    /// Failing balance checks: the panel's count and the status bar's.
    pub fn failing(&self) -> usize {
        self.data.review.failing()
    }

    // Opening a queue ----------------------------------------------------------

    /// Opens `queue` (CHECKS in the panel), selecting its first pair or
    /// its first failing check when nothing in it is selected (user,
    /// 2026-11-14).
    pub fn show(&mut self, queue: Queue, cx: &mut Context<Self>) {
        let opened = self.queue != Some(queue);
        self.queue = Some(queue);
        if opened {
            self.reload(cx);
        }
        self.select_default();
        cx.emit(ChecksEvent::Shown(Some(queue)));
        cx.notify();
    }

    /// Closes the queues: the Inbox shows its suggestions.
    pub fn hide(&mut self, cx: &mut Context<Self>) {
        if self.queue.take().is_some() {
            self.outcome = None;
            cx.emit(ChecksEvent::Shown(None));
            cx.notify();
        }
    }

    /// Opens Balance checks on `account`'s first failing check (the
    /// Accounts panel's flagged dot), else its newest check.
    pub fn show_account(&mut self, account: AccountId, cx: &mut Context<Self>) {
        self.queue = Some(Queue::Balances);
        self.reload(cx);
        let row = self.data.review.account(account).and_then(|review| {
            review
                .rows
                .iter()
                .find(|row| row.check.outcome.is_failed())
                .or_else(|| review.rows.last())
                .map(|row| row.check.balance)
        });
        self.check = row.map(|balance| (account, balance));
        cx.emit(ChecksEvent::Shown(Some(Queue::Balances)));
        cx.notify();
    }

    fn select_default(&mut self) {
        match self.queue {
            Some(Queue::Duplicates) if self.selected_pair().is_none() => {
                self.pair = self.data.review.pairs.first().map(|pair| pair.candidate);
                self.swapped = None;
            }
            Some(Queue::Balances) if self.selected_check().is_none() => {
                self.check = self.data.review.accounts.iter().find_map(|account| {
                    account
                        .rows
                        .iter()
                        .rev()
                        .find(|row| row.check.outcome.is_failed())
                        .map(|row| (account.account, row.check.balance))
                });
            }
            _ => {}
        }
    }

    // Reading ------------------------------------------------------------------

    /// Reads the queues again, keeping the selection where it still is.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.load();
        cx.notify();
    }

    fn load(&mut self) {
        let Source::Database(store) = &self.source else {
            return;
        };
        match store.load() {
            Ok(data) => {
                self.data = data;
                self.load_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't read the review queues");
                self.load_error = Some(error.to_string().into());
            }
        }
        if self
            .pair
            .is_some_and(|id| self.data.review.pair(id).is_none())
        {
            self.pair = None;
        }
        // A swap is planned against the data it was read with.
        if let Some(swapped) = self.swapped.take()
            && Some(swapped.candidate) == self.pair
        {
            self.swap_to(swapped.default_keep());
        }
        if self.selected_check().is_none() {
            self.check = None;
        }
    }

    fn store(&self) -> Result<&ChecksStore, SharedString> {
        match &self.source {
            Source::Database(store) => Ok(store),
            Source::None => Err("There is no database to change.".into()),
        }
    }

    // Possible duplicates ------------------------------------------------------

    /// The pairs in queue order.
    pub fn pairs(&self) -> &[Pair] {
        &self.data.review.pairs
    }

    /// The selected pair as it would merge now: with its rows swapped when
    /// the user chose to keep the other one.
    pub fn selected_pair(&self) -> Option<&Pair> {
        let id = self.pair?;
        match &self.swapped {
            Some(swapped) if swapped.candidate == id => Some(swapped),
            _ => self.data.review.pair(id),
        }
    }

    /// Selects pair `id` (`None`: nothing), or nothing if it's selected.
    pub fn toggle_pair(&mut self, id: DuplicateCandidateId, cx: &mut Context<Self>) {
        let next = (self.pair != Some(id)).then_some(id);
        self.select_pair(next, cx);
    }

    pub fn select_pair(&mut self, id: Option<DuplicateCandidateId>, cx: &mut Context<Self>) {
        if self.pair != id {
            self.pair = id;
            self.swapped = None;
            cx.notify();
        }
    }

    /// Moves the selected pair `step` rows (↑↓), stopping at the ends.
    pub fn select_next_pair(&mut self, step: isize, cx: &mut Context<Self>) {
        let order: Vec<DuplicateCandidateId> =
            self.pairs().iter().map(|pair| pair.candidate).collect();
        let next = step_in(&order, self.pair, step);
        if next.is_some() {
            self.select_pair(next, cx);
        }
    }

    /// "Keep this instead" (←→): plans the selected pair keeping the row
    /// it would exclude; again, back to the default.
    pub fn keep_other(&mut self, cx: &mut Context<Self>) {
        let Some(pair) = self.selected_pair() else {
            return;
        };
        let other = pair.rows[1].transaction;
        self.swap_to(other);
        cx.notify();
    }

    fn swap_to(&mut self, keep: TransactionId) {
        let Some(id) = self.pair else {
            return;
        };
        let Some(listed) = self.data.review.pair(id) else {
            return;
        };
        if listed.default_keep() == keep {
            self.swapped = None;
            return;
        }
        let planned = self.store().and_then(|store| {
            store
                .pair(id, keep)
                .map_err(|error| SharedString::from(error.to_string()))
        });
        match planned {
            Ok(pair) => self.swapped = Some(pair),
            Err(message) => self.outcome = Some(Outcome::Refused(message)),
        }
    }

    /// Whether Merge can go ahead for the selected pair: it has a plan and
    /// the row it excludes isn't locked.
    pub fn can_merge(&self) -> bool {
        self.selected_pair()
            .is_some_and(|pair| pair.effect.as_ref().is_ok_and(|e| e.locked.is_none()))
    }

    /// Merges the selected pair (Merge, M), keeping the row shown as kept.
    pub fn merge(&mut self, cx: &mut Context<Self>) {
        if !self.can_merge() {
            return;
        }
        let Some(pair) = self.selected_pair() else {
            return;
        };
        let (id, keep) = (pair.candidate, pair.default_keep());
        let message = format!(
            "Merged {}: {} excluded as a duplicate",
            text::name(&pair.rows[0]),
            text::name(&pair.rows[1])
        );
        let next = self.next_pair_after(id);
        let result = self.store().and_then(|store| {
            store
                .merge(id, keep)
                .map_err(|error| SharedString::from(error.to_string()))
        });
        self.outcome = Some(match result {
            Ok(merged) => {
                self.pair = next;
                self.swapped = None;
                Outcome::Done {
                    message: message.into(),
                    undo: Some(Undo::Merge(merged.batch, id)),
                }
            }
            Err(message) => Outcome::Refused(message),
        });
        self.reload(cx);
        cx.emit(ChecksEvent::Changed);
    }

    /// "Not duplicates" (N): dismisses the selected pair. It changes no
    /// transaction, so a lock doesn't stop it.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        let Some(pair) = self.selected_pair() else {
            return;
        };
        let id = pair.candidate;
        let message = format!(
            "{} and {} marked as not duplicates",
            text::name(&pair.rows[0]),
            text::name(&pair.rows[1])
        );
        let next = self.next_pair_after(id);
        let result = self.store().and_then(|store| {
            store
                .dismiss(id)
                .map_err(|error| SharedString::from(error.to_string()))
        });
        self.outcome = Some(match result {
            Ok(()) => {
                self.pair = next;
                self.swapped = None;
                Outcome::Done {
                    message: message.into(),
                    undo: Some(Undo::Dismiss(id)),
                }
            }
            Err(message) => Outcome::Refused(message),
        });
        self.reload(cx);
    }

    /// The bar's Undo: undoes the last merge or dismissal, and opens the
    /// pair again.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(Outcome::Done {
            undo: Some(undo), ..
        }) = self.outcome.clone()
        else {
            return;
        };
        let result = self.store().and_then(|store| {
            match undo {
                Undo::Merge(batch, _) => store.undo(batch).map(drop),
                Undo::Dismiss(id) => store.reopen(id),
            }
            .map_err(|error| SharedString::from(error.to_string()))
        });
        let candidate = match undo {
            Undo::Merge(_, id) | Undo::Dismiss(id) => id,
        };
        match result {
            Ok(()) => {
                self.outcome = None;
                self.reload(cx);
                if self.data.review.pair(candidate).is_some() {
                    self.pair = Some(candidate);
                    self.swapped = None;
                }
                if matches!(undo, Undo::Merge(..)) {
                    cx.emit(ChecksEvent::Changed);
                }
            }
            Err(message) => {
                self.outcome = Some(Outcome::Refused(message));
                cx.notify();
            }
        }
    }

    /// The pair to open after `id` leaves the queue: the next one, else the
    /// one before.
    fn next_pair_after(&self, id: DuplicateCandidateId) -> Option<DuplicateCandidateId> {
        let order: Vec<DuplicateCandidateId> =
            self.pairs().iter().map(|pair| pair.candidate).collect();
        let at = order.iter().position(|row| *row == id)?;
        order
            .get(at + 1)
            .or_else(|| at.checked_sub(1).and_then(|before| order.get(before)))
            .copied()
    }

    /// Hides the bar.
    pub fn dismiss_outcome(&mut self, cx: &mut Context<Self>) {
        if self.outcome.take().is_some() {
            cx.notify();
        }
    }

    // Balance checks -----------------------------------------------------------

    pub fn accounts(&self) -> &[AccountReview] {
        &self.data.review.accounts
    }

    /// The selected balance and its account.
    pub fn selected_check(&self) -> Option<(&AccountReview, &CheckRow)> {
        let (account, balance) = self.check?;
        let review = self.data.review.account(account)?;
        let row = review
            .rows
            .iter()
            .find(|row| row.check.balance == balance)?;
        Some((review, row))
    }

    pub fn is_check_selected(&self, account: AccountId, balance: BalanceRef) -> bool {
        self.check == Some((account, balance))
    }

    /// Selects a balance, or nothing if it's selected.
    pub fn toggle_check(
        &mut self,
        account: AccountId,
        balance: BalanceRef,
        cx: &mut Context<Self>,
    ) {
        let next = (self.check != Some((account, balance))).then_some((account, balance));
        self.check = next;
        cx.notify();
    }

    /// The balances in table order: accounts as listed, each newest first.
    fn check_order(&self) -> Vec<(AccountId, BalanceRef)> {
        self.accounts()
            .iter()
            .flat_map(|account| {
                account
                    .rows
                    .iter()
                    .rev()
                    .map(move |row| (account.account, row.check.balance))
            })
            .collect()
    }

    /// Moves the selected balance `step` rows (↑↓), stopping at the ends.
    pub fn select_next_check(&mut self, step: isize, cx: &mut Context<Self>) {
        let order = self.check_order();
        if let Some(next) = step_in(&order, self.check, step) {
            self.check = Some(next);
            cx.notify();
        }
    }

    /// "Review pair": opens the pair in Possible duplicates.
    pub fn review_pair(&mut self, id: DuplicateCandidateId, cx: &mut Context<Self>) {
        self.queue = Some(Queue::Duplicates);
        self.pair = Some(id);
        self.swapped = None;
        cx.emit(ChecksEvent::Shown(Some(Queue::Duplicates)));
        cx.notify();
    }

    /// Clears the selection in the open queue (Escape).
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        match self.queue {
            Some(Queue::Duplicates) => self.select_pair(None, cx),
            Some(Queue::Balances) => {
                self.check = None;
                cx.notify();
            }
            None => {}
        }
    }

    pub fn request_lock(&mut self, account: AccountId, cx: &mut Context<Self>) {
        cx.emit(ChecksEvent::LockRequested(account));
    }

    pub fn request_unlock(&mut self, account: AccountId, cx: &mut Context<Self>) {
        cx.emit(ChecksEvent::UnlockRequested(account));
    }

    pub fn request_add_balance(&mut self, account: AccountId, cx: &mut Context<Self>) {
        cx.emit(ChecksEvent::AddBalanceRequested(account));
    }

    /// Locks `account` through `through` (the lock dialog), or says why
    /// core refused.
    pub fn lock(
        &mut self,
        account: AccountId,
        through: chrono::NaiveDate,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .lock(account, through)
            .map_err(|error| SharedString::from(error.to_string()))?;
        self.reload(cx);
        cx.emit(ChecksEvent::Changed);
        Ok(())
    }

    /// Unlocks `account` (the unlock dialog): the lock is cleared entirely.
    pub fn unlock(
        &mut self,
        account: AccountId,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?
            .unlock(account)
            .map_err(|error| SharedString::from(error.to_string()))?;
        // Reading again plans the selected pair again, lock lifted.
        self.reload(cx);
        cx.emit(ChecksEvent::Changed);
        Ok(())
    }

    /// The account named `account`, for dialogs.
    pub fn account(&self, account: AccountId) -> Option<&AccountReview> {
        self.data.review.account(account)
    }
}

/// The item `step` rows from `current` in `order`, stopping at the ends;
/// with nothing selected, down gives the first and up the last.
fn step_in<T: Copy + PartialEq>(order: &[T], current: Option<T>, step: isize) -> Option<T> {
    if order.is_empty() {
        return None;
    }
    let next = match current.and_then(|id| order.iter().position(|row| *row == id)) {
        Some(at) => at.saturating_add_signed(step).min(order.len() - 1),
        None if step < 0 => order.len() - 1,
        None => 0,
    };
    Some(order[next])
}

#[cfg(test)]
mod tests;
