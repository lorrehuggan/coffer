//! The Inbox (ROADMAP M3.6, M3.14; `docs/DESIGN.md` → "Inbox", "Inbox
//! impact preview"; `docs/ARCHITECTURE.md` → "Decision record: Inbox"):
//! the suggestions AI clients (and Coffer itself) filed, grouped by kind,
//! each with what approving it would change, row by row, before it can be
//! approved.
//!
//! - [`Inbox`] holds what the screen shows (from [`store::InboxStore`],
//!   which opens the database per call), the filters, the selected
//!   suggestion and the outcome of the last decision. Every decision is
//!   core's ([`coffer_core::inbox`]): this only picks the IDs.
//! - [`panel`]: the left panel (INBOX status, KIND, SOURCE).
//! - [`view::InboxView`]: the toolbar, the grouped table with the selected
//!   suggestion open in place, and the inspector (`inspector`).
//!
//! Approval happens only here and in the CLI, never over MCP (`AGENTS.md`
//! → Invariants). A rule approved here is applied to past transactions
//! too, never over a category the user set (decision record "Inbox impact
//! preview", point 1), and "Approve all" leaves wide rules out (point 3).
//!
//! Rationales, patterns, names and bank text are untrusted text: shown as
//! plain text, stripped ([`text::plain`]), never acted on.

pub mod fixture;
mod inspector;
pub mod panel;
pub mod store;
pub mod text;
pub mod view;

use coffer_core::inbox::preview::Covered;
use coffer_core::inbox::{COFFER_SOURCE, SuggestionId, SuggestionKind, SuggestionStatus};
use gpui_kit::{Context, SharedString};

use store::{Entry, InboxData, InboxStore};

/// The order the kinds are listed in, widest reach first (`DESIGN.md` →
/// "Inbox impact preview" → "Kinds and order").
pub const KIND_ORDER: [SuggestionKind; 5] = [
    SuggestionKind::Rule,
    SuggestionKind::MerchantAlias,
    SuggestionKind::TransferPair,
    SuggestionKind::RefundLink,
    SuggestionKind::Categorization,
];

/// Binds the Inbox's keys: A approve, R reject, ↑↓ move, Escape clears the
/// selection. Call once, before the first [`view::InboxView`].
pub fn init(cx: &mut gpui_kit::App) {
    use gpui_kit::KeyBinding;
    use gpui_kit::base::actions::{Cancel, SelectDown, SelectUp};
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, Some(view::CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(view::CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(view::CONTEXT)),
        KeyBinding::new("a", view::ApproveSelected, Some(view::CONTEXT)),
        KeyBinding::new("r", view::RejectSelected, Some(view::CONTEXT)),
    ]);
}

/// Where the suggestions come from.
enum Source {
    Database(InboxStore),
    /// Nothing to read (no data folder, or a test without a database).
    None,
}

/// A group of the table: one kind from one source.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GroupKey {
    pub kind: SuggestionKind,
    pub source: String,
}

/// What the last decision did, shown until the next one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It went through: what changed, for the confirmation bar.
    Done(SharedString),
    /// Core refused it; nothing changed. Shown where it was asked: under
    /// the inspector's card or the group's buttons.
    Refused { scope: Scope, message: SharedString },
}

/// Where a decision was asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Suggestion(SuggestionId),
    Group(GroupKey),
}

/// The Inbox's state (module docs).
pub struct Inbox {
    source: Source,
    data: InboxData,
    status: SuggestionStatus,
    kind: Option<SuggestionKind>,
    from: Option<String>,
    selected: Option<SuggestionId>,
    /// The pending rule that would settle the selected categorisation,
    /// looked up when it's selected.
    covered: Option<(SuggestionId, Option<Covered>)>,
    outcome: Option<Outcome>,
    load_error: Option<SharedString>,
}

impl Inbox {
    /// The suggestions in `store`, read now.
    pub fn from_store(store: InboxStore) -> Self {
        let mut inbox = Self::with_source(Source::Database(store));
        inbox.load();
        inbox
    }

    /// No suggestions, because they can't be read: `reason` says why.
    pub fn unavailable(reason: impl Into<SharedString>) -> Self {
        Self {
            load_error: Some(reason.into()),
            ..Self::with_source(Source::None)
        }
    }

    /// An empty Inbox that reads nothing (tests).
    pub fn empty() -> Self {
        Self::with_source(Source::None)
    }

    fn with_source(source: Source) -> Self {
        Self {
            source,
            data: InboxData::default(),
            status: SuggestionStatus::Pending,
            kind: None,
            from: None,
            selected: None,
            covered: None,
            outcome: None,
            load_error: None,
        }
    }

    pub fn data(&self) -> &InboxData {
        &self.data
    }

    pub fn status(&self) -> SuggestionStatus {
        self.status
    }

    pub fn kind_filter(&self) -> Option<SuggestionKind> {
        self.kind
    }

    pub fn source_filter(&self) -> Option<&str> {
        self.from.as_deref()
    }

    pub fn selected(&self) -> Option<SuggestionId> {
        self.selected
    }

    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    pub fn load_error(&self) -> Option<&SharedString> {
        self.load_error.as_ref()
    }

    /// The count on the Inbox tab: pending suggestions from AI clients.
    /// Coffer's own aren't counted (`DESIGN.md` → "Inbox impact preview").
    pub fn badge(&self) -> usize {
        let own = self
            .data
            .counts
            .by_source
            .get(COFFER_SOURCE)
            .copied()
            .unwrap_or(0);
        self.data.counts.pending.saturating_sub(own)
    }

    /// The covering rule for the selected categorisation, if it's known.
    pub fn covered(&self) -> Option<&Covered> {
        match (&self.covered, self.selected) {
            (Some((id, covered)), Some(selected)) if *id == selected => covered.as_ref(),
            _ => None,
        }
    }

    /// The suggestions the filters show, as table groups: kinds in
    /// [`KIND_ORDER`], then sources by name with Coffer's last.
    pub fn groups(&self) -> Vec<(GroupKey, Vec<&Entry>)> {
        let mut groups: Vec<(GroupKey, Vec<&Entry>)> = Vec::new();
        for kind in KIND_ORDER {
            if self.kind.is_some_and(|filter| filter != kind) {
                continue;
            }
            let mut sources: Vec<&str> = self
                .data
                .entries
                .iter()
                .filter(|entry| entry.suggestion.kind == kind)
                .map(|entry| entry.suggestion.source_client.as_str())
                .filter(|source| self.from.as_deref().is_none_or(|from| from == *source))
                .collect();
            sources.sort_by_key(|source| (*source == COFFER_SOURCE, *source));
            sources.dedup();
            for source in sources {
                let entries = self
                    .data
                    .entries
                    .iter()
                    .filter(|entry| {
                        entry.suggestion.kind == kind && entry.suggestion.source_client == source
                    })
                    .collect();
                groups.push((
                    GroupKey {
                        kind,
                        source: source.to_owned(),
                    },
                    entries,
                ));
            }
        }
        groups
    }

    /// The shown suggestions in table order.
    pub fn order(&self) -> Vec<SuggestionId> {
        self.groups()
            .into_iter()
            .flat_map(|(_, entries)| entries.into_iter().map(Entry::id))
            .collect()
    }

    pub fn entry(&self, id: SuggestionId) -> Option<&Entry> {
        self.data.entries.iter().find(|entry| entry.id() == id)
    }

    /// The selected suggestion, if it's shown.
    pub fn selected_entry(&self) -> Option<&Entry> {
        self.selected.and_then(|id| self.entry(id))
    }

    /// Shows the suggestions with `status`.
    pub fn show_status(&mut self, status: SuggestionStatus, cx: &mut Context<Self>) {
        if self.status == status {
            return;
        }
        self.status = status;
        self.selected = None;
        self.outcome = None;
        self.reload(cx);
    }

    /// Shows only `kind`, or every kind if it's shown alone already.
    pub fn toggle_kind(&mut self, kind: SuggestionKind, cx: &mut Context<Self>) {
        self.kind = (self.kind != Some(kind)).then_some(kind);
        self.keep_selection_visible();
        cx.notify();
    }

    /// Shows only `source`, or every source if it's shown alone already.
    pub fn toggle_source(&mut self, source: &str, cx: &mut Context<Self>) {
        self.from = (self.from.as_deref() != Some(source)).then(|| source.to_owned());
        self.keep_selection_visible();
        cx.notify();
    }

    fn keep_selection_visible(&mut self) {
        if self.selected.is_some_and(|id| !self.order().contains(&id)) {
            self.selected = None;
        }
    }

    /// Selects `id` (`None`: nothing), or clears it if it's selected.
    pub fn select(&mut self, id: Option<SuggestionId>, cx: &mut Context<Self>) {
        if self.selected == id {
            return;
        }
        self.selected = id;
        self.lookup_covered();
        cx.notify();
    }

    /// A click on a row: selects it, or clears the selection if it's
    /// selected already.
    pub fn toggle(&mut self, id: SuggestionId, cx: &mut Context<Self>) {
        let next = (self.selected != Some(id)).then_some(id);
        self.select(next, cx);
    }

    /// Moves the selection `step` rows (↑↓), stopping at the ends; with
    /// nothing selected, down selects the first and up the last.
    pub fn select_next(&mut self, step: isize, cx: &mut Context<Self>) {
        let order = self.order();
        if order.is_empty() {
            return;
        }
        let next = match self
            .selected
            .and_then(|id| order.iter().position(|row| *row == id))
        {
            Some(at) => at.saturating_add_signed(step).min(order.len() - 1),
            None if step < 0 => order.len() - 1,
            None => 0,
        };
        self.select(Some(order[next]), cx);
    }

    /// Looks up the rule that would cover the selected categorisation.
    fn lookup_covered(&mut self) {
        let Some(entry) = self.selected_entry() else {
            return;
        };
        if entry.suggestion.kind != SuggestionKind::Categorization
            || entry.suggestion.status != SuggestionStatus::Pending
        {
            return;
        }
        let id = entry.id();
        if matches!(&self.covered, Some((covered, _)) if *covered == id) {
            return;
        }
        let Source::Database(store) = &self.source else {
            return;
        };
        let covered = store.covered_by(id).unwrap_or_else(|error| {
            tracing::warn!(%error, "couldn't look up a covering rule");
            None
        });
        self.covered = Some((id, covered));
    }

    /// Reads the suggestions again.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.load();
        cx.notify();
    }

    fn load(&mut self) {
        let Source::Database(store) = &self.source else {
            return;
        };
        match store.load(self.status) {
            Ok(data) => {
                self.data = data;
                self.load_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't read the inbox");
                self.load_error = Some(error.to_string().into());
            }
        }
        self.covered = None;
        self.keep_selection_visible();
        if self.selected.is_some_and(|id| self.entry(id).is_none()) {
            self.selected = None;
        }
        self.lookup_covered();
    }

    fn store(&self) -> Result<&InboxStore, SharedString> {
        match &self.source {
            Source::Database(store) => Ok(store),
            Source::None => Err("There is no database to change.".into()),
        }
    }

    /// Approves `id` on its own (its row's ✓, the inspector, A).
    pub fn approve(&mut self, id: SuggestionId, cx: &mut Context<Self>) {
        self.decide(Scope::Suggestion(id), vec![id], true, cx);
    }

    /// Rejects `id` (its row's ✕, the inspector, R).
    pub fn reject(&mut self, id: SuggestionId, cx: &mut Context<Self>) {
        self.decide(Scope::Suggestion(id), vec![id], false, cx);
    }

    /// The suggestions "Approve all" approves in `group`: all but the ones
    /// that need their own approval (wide rules).
    pub fn approvable(&self, group: &GroupKey) -> Vec<SuggestionId> {
        self.groups()
            .into_iter()
            .filter(|(key, _)| key == group)
            .flat_map(|(_, entries)| entries)
            .filter(|entry| !entry.needs_own_approval())
            .map(Entry::id)
            .collect()
    }

    /// Every suggestion shown in `group`.
    pub fn in_group(&self, group: &GroupKey) -> Vec<SuggestionId> {
        self.groups()
            .into_iter()
            .filter(|(key, _)| key == group)
            .flat_map(|(_, entries)| entries.into_iter().map(Entry::id))
            .collect()
    }

    /// "Approve all (n)": every [`approvable`](Self::approvable) one, all
    /// or none.
    pub fn approve_group(&mut self, group: GroupKey, cx: &mut Context<Self>) {
        let ids = self.approvable(&group);
        if !ids.is_empty() {
            self.decide(Scope::Group(group), ids, true, cx);
        }
    }

    /// "Reject all": every suggestion in `group`, all or none.
    pub fn reject_group(&mut self, group: GroupKey, cx: &mut Context<Self>) {
        let ids = self.in_group(&group);
        if !ids.is_empty() {
            self.decide(Scope::Group(group), ids, false, cx);
        }
    }

    fn decide(
        &mut self,
        scope: Scope,
        ids: Vec<SuggestionId>,
        approve: bool,
        cx: &mut Context<Self>,
    ) {
        if self.status != SuggestionStatus::Pending {
            return;
        }
        let next = self.next_after(&ids);
        let message = approve
            .then(|| text::approved_message(&self.data, &ids))
            .flatten();
        let result = self.store().and_then(|store| {
            if approve {
                store.approve(&ids).map(drop)
            } else {
                store.reject(&ids)
            }
            .map_err(|error| SharedString::from(error.to_string()))
        });
        self.outcome = match result {
            Ok(()) => {
                self.selected = next;
                message.map(|message| Outcome::Done(message.into()))
            }
            Err(message) => Some(Outcome::Refused { scope, message }),
        };
        self.reload(cx);
    }

    /// The row to select after `ids` leave the list: the next one below
    /// the last of them, else the one above the first.
    fn next_after(&self, ids: &[SuggestionId]) -> Option<SuggestionId> {
        let order = self.order();
        let selected = self.selected?;
        if !ids.contains(&selected) {
            return Some(selected);
        }
        let at = order.iter().position(|id| *id == selected)?;
        order[at..]
            .iter()
            .chain(order[..at].iter().rev())
            .find(|id| !ids.contains(id))
            .copied()
    }

    /// Hides the confirmation bar.
    pub fn dismiss_outcome(&mut self, cx: &mut Context<Self>) {
        if self.outcome.take().is_some() {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests;
