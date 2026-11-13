//! Applying what the rules set ([`Outcome`]) to stored transactions: to an
//! import batch ([`apply_to_batch`](super::apply_to_batch)), and to past
//! transactions as one undoable change batch ([`preview_apply`],
//! [`commit_apply`]).
//!
//! What each action writes, which values count as the user's, and how
//! Apply works are in the [`rule`](crate::rule) module docs ("Applying").

use std::collections::{BTreeSet, HashMap};

use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

use super::ActionKind;
use super::allocate::{share_minor, split_amounts};
use super::engine::{MatchInput, Outcome, RuleSet, SetBy};
use super::store::{InvalidRule, StoreError};
use crate::category::CategoryKey;
use crate::change::{
    self, CategorySource, CategoryValue, ChangeKind, ExclusionValue, Field, MerchantValue,
    NameSource, Recorder, ShareValue, SplitLineValue, Value,
};
use crate::db::Database;
use crate::exclusion::ExclusionReason;
use crate::id::{AccountId, BatchId, ChangeBatchId, RuleId, TagId, TransactionId};
use crate::merchant::DisplayName;
use crate::note::UserNote;
use crate::tag::{self, TagName};

// Plans -----------------------------------------------------------------------

/// What a change sets a field to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A category, set by the rule.
    Category {
        /// The category's key.
        key: CategoryKey,
        /// Its row ID.
        id: i64,
    },
    /// A split into these lines. It clears the row's category and share.
    Split {
        /// The lines, in the template's order.
        lines: Vec<PlannedLine>,
        /// The share (and label) the split clears, as it is now.
        clears_share: ShareValue,
    },
    /// A display merchant set by a rule (trimmed).
    Merchant(String),
    /// Tags to add, which the row doesn't have yet (ignoring case).
    Tags(Vec<TagName>),
    /// A user note (trimmed).
    Note(String),
    /// An exclusion for this reason, with no note.
    Exclude(ExclusionReason),
    /// The row's share; the label is kept as it is.
    Share(ShareValue),
}

impl Target {
    /// The field the target is for. A split is [`Field::Split`], though it
    /// also clears the category and the share.
    pub const fn field(&self) -> Field {
        match self {
            Target::Category { .. } => Field::Category,
            Target::Split { .. } => Field::Split,
            Target::Merchant(_) => Field::Merchant,
            Target::Tags(_) => Field::Tags,
            Target::Note(_) => Field::UserNote,
            Target::Exclude(_) => Field::Exclusion,
            Target::Share(_) => Field::Share,
        }
    }
}

/// One line of a planned split.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedLine {
    /// The line's category key.
    pub category: CategoryKey,
    /// Its row ID.
    pub category_id: i64,
    /// The line's amount: non-zero, with the row's sign.
    pub amount_minor: i64,
    /// The user's share of the line (`None`: all of it).
    pub my_share_minor: Option<i64>,
}

/// One field a rule changes on one row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedChange {
    /// The rule that sets it.
    pub rule: RuleId,
    /// What the field holds now. For a split: the row's category.
    pub old: Value,
    /// What it becomes.
    pub new: Target,
    /// Whether it overwrites a value the user set ([`rule`](crate::rule)
    /// module docs, "Values the user set"): made only
    /// on rows the user includes, never on import.
    pub user_set: bool,
    /// The lock date it meets: `Some(reconciled_through)` for an exclusion
    /// change on a row dated on or before it. Applying it needs an unlock.
    pub locked: Option<NaiveDate>,
}

impl PlannedChange {
    /// Whether the change is made, given whether its row is included.
    pub const fn applies(&self, included: bool) -> bool {
        !self.user_set || included
    }
}

/// What the rules change on one row: one entry per field (tags: per rule),
/// in the order they are made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowPlan {
    /// The transaction.
    pub transaction: TransactionId,
    /// The changes; never empty in a preview.
    pub changes: Vec<PlannedChange>,
}

impl RowPlan {
    /// Whether some change overwrites a value the user set, so the row is
    /// one the user can include.
    pub fn needs_inclusion(&self) -> bool {
        self.changes.iter().any(|change| change.user_set)
    }

    /// Whether some change is made without inclusion.
    pub fn has_automatic(&self) -> bool {
        self.changes.iter().any(|change| !change.user_set)
    }

    /// The lock a change on the row meets, if any.
    pub fn locked(&self) -> Option<NaiveDate> {
        self.changes.iter().find_map(|change| change.locked)
    }

    /// Whether a change made with the row `included` (or not) meets a lock.
    fn locked_if(&self, included: bool) -> bool {
        self.changes
            .iter()
            .any(|change| change.locked.is_some() && change.applies(included))
    }
}

/// Why a split rule didn't split a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SplitSkip {
    /// The row is in a suggested or confirmed transfer, which can't be
    /// split.
    InTransfer,
    /// The amount is zero or too small to give every line at least one
    /// minor unit.
    TooSmall,
    /// The row is money in with a suggested or confirmed refund,
    /// reimbursement or settlement link: its linked part takes the
    /// purchase's category and share, so it is neither split nor shared
    /// (M1.25). A share the rules give it is skipped too.
    Linked,
}

/// A row a split rule won but didn't split. The row keeps its category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkippedSplit {
    /// The transaction.
    pub transaction: TransactionId,
    /// The rule with the split.
    pub rule: RuleId,
    /// Why it wasn't split.
    pub reason: SplitSkip,
}

/// A transaction as the planner reads it.
struct Candidate {
    id: TransactionId,
    account_id: AccountId,
    amount_minor: i64,
    merchant: Option<String>,
    description: String,
    source_category: Option<String>,
    source_type: Option<String>,
    /// Whether it has split lines.
    split: bool,
    /// Whether it is in a suggested or confirmed transfer group.
    in_transfer: bool,
    /// Whether it is the money in of a suggested or confirmed link.
    linked: bool,
}

impl Candidate {
    fn input(&self) -> MatchInput<'_> {
        MatchInput {
            account_id: self.account_id,
            amount_minor: self.amount_minor,
            merchant: self.merchant.as_deref(),
            description: &self.description,
            source_category: self.source_category.as_deref(),
            source_type: self.source_type.as_deref(),
        }
    }
}

/// The transactions `filter` (a condition on `"transaction" t`) selects, by
/// ID.
fn load_candidates(
    conn: &Connection,
    filter: &str,
    params: impl rusqlite::Params,
) -> rusqlite::Result<Vec<Candidate>> {
    let mut statement = conn.prepare(&format!(
        "SELECT t.id, t.account_id, t.amount_minor, t.merchant, t.description_raw, \
         t.source_category, t.source_type, \
         EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id), \
         coalesce(g.status IN ('suggested', 'confirmed'), 0), \
         EXISTS (SELECT 1 FROM transaction_link l \
                 WHERE l.inflow_id = t.id AND l.status <> 'rejected') \
         FROM \"transaction\" t LEFT JOIN transfer_group g ON g.id = t.transfer_group_id \
         WHERE {filter} ORDER BY t.id"
    ))?;
    statement
        .query_map(params, |row| {
            Ok(Candidate {
                id: row.get(0)?,
                account_id: row.get(1)?,
                amount_minor: row.get(2)?,
                merchant: row.get(3)?,
                description: row.get(4)?,
                source_category: row.get(5)?,
                source_type: row.get(6)?,
                split: row.get(7)?,
                in_transfer: row.get(8)?,
                linked: row.get(9)?,
            })
        })?
        .collect()
}

/// A stored rule whose action can't be applied: an error naming the rule
/// and the action's kind, never its text.
fn invalid(rule: RuleId, kind: ActionKind) -> StoreError {
    StoreError::Invalid {
        id: rule,
        problem: InvalidRule::Action(kind),
    }
}

/// Plans rows, caching category IDs.
struct Planner<'c> {
    conn: &'c Connection,
    category_ids: HashMap<CategoryKey, i64>,
}

impl<'c> Planner<'c> {
    fn new(conn: &'c Connection) -> Planner<'c> {
        Planner {
            conn,
            category_ids: HashMap::new(),
        }
    }

    /// The ID of the category `rule` names.
    fn category_id(&mut self, key: &CategoryKey, rule: RuleId) -> Result<i64, StoreError> {
        if let Some(id) = self.category_ids.get(key) {
            return Ok(*id);
        }
        let id = self
            .conn
            .query_row("SELECT id FROM category WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?
            .ok_or_else(|| StoreError::RuleCategoryMissing {
                rule,
                key: key.clone(),
            })?;
        self.category_ids.insert(key.clone(), id);
        Ok(id)
    }

    /// What `outcome` changes on `row`, which isn't split, and the split it
    /// skips, if any.
    fn plan_row(
        &mut self,
        row: &Candidate,
        outcome: &Outcome,
    ) -> Result<(Vec<PlannedChange>, Option<SkippedSplit>), StoreError> {
        let conn = self.conn;
        let id = row.id;
        let mut changes = Vec::new();
        let mut push = |rule, old, new, user_set, locked| {
            changes.push(PlannedChange {
                rule,
                old,
                new,
                user_set,
                locked,
            });
        };

        if let Some(SetBy { value, rule }) = &outcome.merchant {
            let name = DisplayName::new(value.as_str())
                .map_err(|_| invalid(*rule, ActionKind::SetMerchant))?
                .as_str()
                .to_owned();
            let now = read_merchant(conn, id)?;
            let user_set = now.source == Some(NameSource::User);
            let same = now.name.as_deref() == Some(name.as_str())
                && (user_set || now.source == Some(NameSource::Rule));
            if !same {
                push(
                    *rule,
                    Value::Merchant(now),
                    Target::Merchant(name),
                    user_set,
                    None,
                );
            }
        }

        if let Some(SetBy { value, rule }) = &outcome.note {
            let note = UserNote::new(value.as_str())
                .ok()
                .flatten()
                .ok_or_else(|| invalid(*rule, ActionKind::SetNote))?;
            let now = read_note(conn, id)?;
            if now.as_deref() != Some(note.as_str()) {
                let user_set = now.is_some();
                push(
                    *rule,
                    Value::UserNote(now),
                    Target::Note(note.as_str().to_owned()),
                    user_set,
                    None,
                );
            }
        }

        if !outcome.tags.is_empty() {
            let have = tag_names(conn, id)?;
            let mut by_rule: Vec<(RuleId, Vec<TagName>)> = Vec::new();
            for SetBy { value, rule } in &outcome.tags {
                if have.contains(&value.folded()) {
                    continue;
                }
                match by_rule.iter_mut().find(|(by, _)| by == rule) {
                    Some((_, names)) => names.push(value.clone()),
                    None => by_rule.push((*rule, vec![value.clone()])),
                }
            }
            if !by_rule.is_empty() {
                let now = change::read(conn, id, Field::Tags)?;
                for (rule, names) in by_rule {
                    push(rule, now.clone(), Target::Tags(names), false, None);
                }
            }
        }

        if let Some(SetBy { value, rule }) = &outcome.exclude {
            let reason = value.exclusion_reason();
            if reason.needs_note() {
                return Err(invalid(*rule, ActionKind::Exclude));
            }
            let now = read_exclusion(conn, id)?;
            if now.as_ref().map(|exclusion| exclusion.reason) != Some(reason) {
                let user_set = now.is_some();
                let locked = change::locked(conn, id, Field::Exclusion)?;
                push(
                    *rule,
                    Value::Exclusion(now),
                    Target::Exclude(reason),
                    user_set,
                    locked,
                );
            }
        }

        let category = read_category(conn, id)?;
        let user_category = matches!(
            category.source,
            Some(CategorySource::User | CategorySource::AiApproved)
        );
        if let Some(SetBy { value: key, rule }) = &outcome.category {
            let category_id = self.category_id(key, *rule)?;
            let target = CategoryValue {
                category_id: Some(category_id),
                source: Some(CategorySource::Rule),
                rule_id: Some(*rule),
            };
            let same =
                category == target || (user_category && category.category_id == Some(category_id));
            if !same {
                push(
                    *rule,
                    Value::Category(category.clone()),
                    Target::Category {
                        key: key.clone(),
                        id: category_id,
                    },
                    user_category,
                    None,
                );
            }
        }

        let mut skipped = None;
        let mut split = false;
        if let Some(SetBy {
            value: template,
            rule,
        }) = &outcome.split
        {
            let amounts = split_amounts(row.amount_minor, template);
            match amounts {
                _ if row.in_transfer => {
                    skipped = Some((*rule, SplitSkip::InTransfer));
                }
                _ if row.linked => skipped = Some((*rule, SplitSkip::Linked)),
                None => skipped = Some((*rule, SplitSkip::TooSmall)),
                Some(amounts) => {
                    let share = outcome.share.as_ref().map(|set| set.value);
                    let mut lines = Vec::with_capacity(amounts.len());
                    for (line, amount_minor) in template.lines().iter().zip(amounts) {
                        lines.push(PlannedLine {
                            category: line.category().clone(),
                            category_id: self.category_id(line.category(), *rule)?,
                            amount_minor,
                            my_share_minor: share
                                .and_then(|share| share_minor(amount_minor, share)),
                        });
                    }
                    let share_now = read_share(conn, id)?;
                    let user_set = user_category || share_now != ShareValue::WHOLE;
                    push(
                        *rule,
                        Value::Category(category.clone()),
                        Target::Split {
                            lines,
                            clears_share: share_now,
                        },
                        user_set,
                        None,
                    );
                    split = true;
                }
            }
        }

        if let Some(SetBy { value: share, rule }) = &outcome.share
            && !split
            && !row.linked
        {
            let now = read_share(conn, id)?;
            let target = ShareValue {
                my_share_minor: share_minor(row.amount_minor, *share),
                share_label: now.share_label.clone(),
            };
            if now != target {
                let user_set = now.my_share_minor.is_some();
                push(
                    *rule,
                    Value::Share(now),
                    Target::Share(target),
                    user_set,
                    None,
                );
            }
        }

        let skipped = skipped.map(|(rule, reason)| SkippedSplit {
            transaction: id,
            rule,
            reason,
        });
        Ok((changes, skipped))
    }
}

fn read_category(conn: &Connection, id: TransactionId) -> Result<CategoryValue, StoreError> {
    match change::read(conn, id, Field::Category)? {
        Value::Category(value) => Ok(value),
        _ => unreachable!("reading a category gives a category"),
    }
}

fn read_merchant(conn: &Connection, id: TransactionId) -> Result<MerchantValue, StoreError> {
    match change::read(conn, id, Field::Merchant)? {
        Value::Merchant(value) => Ok(value),
        _ => unreachable!("reading a merchant gives a merchant"),
    }
}

fn read_note(conn: &Connection, id: TransactionId) -> Result<Option<String>, StoreError> {
    match change::read(conn, id, Field::UserNote)? {
        Value::UserNote(value) => Ok(value),
        _ => unreachable!("reading a note gives a note"),
    }
}

fn read_exclusion(
    conn: &Connection,
    id: TransactionId,
) -> Result<Option<ExclusionValue>, StoreError> {
    match change::read(conn, id, Field::Exclusion)? {
        Value::Exclusion(value) => Ok(value),
        _ => unreachable!("reading an exclusion gives an exclusion"),
    }
}

fn read_share(conn: &Connection, id: TransactionId) -> Result<ShareValue, StoreError> {
    match change::read(conn, id, Field::Share)? {
        Value::Share(value) => Ok(value),
        _ => unreachable!("reading a share gives a share"),
    }
}

fn read_tags(conn: &Connection, id: TransactionId) -> Result<Vec<TagId>, StoreError> {
    match change::read(conn, id, Field::Tags)? {
        Value::Tags(value) => Ok(value),
        _ => unreachable!("reading tags gives tags"),
    }
}

/// The folded names of `transaction`'s tags.
fn tag_names(conn: &Connection, transaction: TransactionId) -> rusqlite::Result<BTreeSet<String>> {
    let mut statement = conn.prepare_cached(
        "SELECT tag.name FROM transaction_tag JOIN tag ON tag.id = transaction_tag.tag_id \
         WHERE transaction_tag.transaction_id = ?1",
    )?;
    statement
        .query_map([transaction], |row| row.get::<_, String>(0))?
        .map(|name| name.map(|name| super::fold(&name)))
        .collect()
}

// Writing ---------------------------------------------------------------------

/// What writing one row's plan changed.
#[derive(Clone, Copy, Debug, Default)]
struct RowWritten {
    changed: bool,
    categorized: bool,
    split: bool,
}

/// Writes plans through a recorder, caching tag IDs by folded name.
#[derive(Default)]
struct Writer {
    tags: HashMap<String, TagId>,
}

impl Writer {
    fn tag_id(&mut self, conn: &Connection, name: &TagName) -> rusqlite::Result<TagId> {
        let folded = name.folded();
        if let Some(id) = self.tags.get(&folded) {
            return Ok(*id);
        }
        let id = tag::find_or_create(conn, name)?;
        self.tags.insert(folded, id);
        Ok(id)
    }

    /// Makes the changes that apply, with the row `included` or not.
    fn write(
        &mut self,
        conn: &Connection,
        transaction: TransactionId,
        changes: &[PlannedChange],
        included: bool,
        recorder: &mut Recorder,
    ) -> Result<RowWritten, StoreError> {
        let mut written = RowWritten::default();
        for change in changes.iter().filter(|change| change.applies(included)) {
            let mut set = |value| recorder.set(conn, transaction, value);
            let changed = match &change.new {
                Target::Category { id, .. } => {
                    let changed = set(Value::Category(CategoryValue {
                        category_id: Some(*id),
                        source: Some(CategorySource::Rule),
                        rule_id: Some(change.rule),
                    }))?;
                    written.categorized |= changed;
                    changed
                }
                Target::Split { lines, .. } => {
                    // A row takes lines only with no category or share of
                    // its own; clearing them first in the same recorder
                    // lets undo restore them after removing the lines.
                    let mut changed = set(Value::Category(CategoryValue::NONE))?;
                    changed |= set(Value::Share(ShareValue::WHOLE))?;
                    let lines = lines
                        .iter()
                        .map(|line| SplitLineValue {
                            amount_minor: line.amount_minor,
                            category_id: Some(line.category_id),
                            my_share_minor: line.my_share_minor,
                            share_label: None,
                            user_note: None,
                        })
                        .collect();
                    let split = set(Value::Split(lines))?;
                    written.split |= split;
                    changed | split
                }
                Target::Merchant(name) => set(Value::Merchant(MerchantValue {
                    name: Some(name.clone()),
                    source: Some(NameSource::Rule),
                }))?,
                Target::Tags(names) => {
                    let mut ids = read_tags(conn, transaction)?;
                    for name in names {
                        ids.push(self.tag_id(conn, name)?);
                    }
                    ids.sort();
                    ids.dedup();
                    set(Value::Tags(ids))?
                }
                Target::Note(note) => set(Value::UserNote(Some(note.clone())))?,
                Target::Exclude(reason) => {
                    set(Value::Exclusion(Some(ExclusionValue::now(*reason))))?
                }
                Target::Share(share) => set(Value::Share(share.clone()))?,
            };
            written.changed |= changed;
        }
        Ok(written)
    }
}

// Import ----------------------------------------------------------------------

/// What the rules did to an import batch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BatchApplied {
    /// Rows a rule changed in any way.
    pub rows: usize,
    /// Rows given a category by a rule.
    pub categorized: usize,
    /// Rows split by a rule.
    pub split: usize,
    /// Rows a split rule won but couldn't split ([`SplitSkip`]).
    pub splits_skipped: usize,
}

/// [`RuleSet::apply_to_batch`]: plans every row of `batch` with the whole
/// rule set and makes the changes that overwrite nothing the user set.
pub(super) fn apply_to_batch(
    rules: &RuleSet,
    conn: &Connection,
    batch: BatchId,
) -> Result<BatchApplied, StoreError> {
    let mut summary = BatchApplied::default();
    if rules.is_empty() {
        return Ok(summary);
    }
    let rows = load_candidates(conn, "t.batch_id = ?1", [batch])?;
    let mut planner = Planner::new(conn);
    let mut writer = Writer::default();
    // Never finished: an import records no change batch, and undoing the
    // import deletes its rows.
    let mut recorder = Recorder::new(ChangeKind::RuleApply);
    for row in rows.iter().filter(|row| !row.split) {
        let outcome = rules.evaluate(&row.input());
        if outcome.is_empty() {
            continue;
        }
        let (changes, skipped) = planner.plan_row(row, &outcome)?;
        summary.splits_skipped += usize::from(skipped.is_some());
        let written = writer.write(conn, row.id, &changes, false, &mut recorder)?;
        summary.rows += usize::from(written.changed);
        summary.categorized += usize::from(written.categorized);
        summary.split += usize::from(written.split);
    }
    if summary.rows > 0 {
        tracing::debug!(
            batch = %batch,
            rows = summary.rows,
            categorized = summary.categorized,
            split = summary.split,
            splits_skipped = summary.splits_skipped,
            "applied rules to an import batch"
        );
    }
    Ok(summary)
}

// Apply -----------------------------------------------------------------------

/// What applying a rule to past transactions would do ([`preview_apply`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyPreview {
    /// The rule.
    pub rule: RuleId,
    /// Every row the rule would change, by transaction ID, with every
    /// change (those that need inclusion too).
    pub rows: Vec<RowPlan>,
    /// Rows the rule matches that are split, and so skipped.
    pub split_rows: Vec<TransactionId>,
    /// Rows whose split the rule wins but can't make.
    pub skipped_splits: Vec<SkippedSplit>,
}

impl ApplyPreview {
    /// Rows with a change made without inclusion: what the apply does when
    /// the user includes nothing.
    pub fn automatic_rows(&self) -> impl Iterator<Item = &RowPlan> {
        self.rows.iter().filter(|row| row.has_automatic())
    }

    /// Rows with a change that overwrites a value the user set, listed
    /// separately: those changes are made only on rows passed to
    /// [`commit_apply`] as included.
    pub fn user_set_rows(&self) -> impl Iterator<Item = &RowPlan> {
        self.rows.iter().filter(|row| row.needs_inclusion())
    }

    /// Rows with an exclusion change in a locked period. The commit refuses
    /// while it would make one: the user unlocks first.
    pub fn locked_rows(&self) -> impl Iterator<Item = &RowPlan> {
        self.rows.iter().filter(|row| row.locked().is_some())
    }

    /// Whether the apply would change nothing, whatever is included.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// What [`commit_apply`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Applied {
    /// The `rule_apply` change batch; `None` when nothing changed.
    pub batch: Option<ChangeBatchId>,
    /// Transactions changed.
    pub transactions: usize,
    /// Fields changed: the batch's items.
    pub items: usize,
}

/// Why applying a rule to past transactions failed. Messages never contain
/// transaction text or rule text.
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    /// Loading, matching or writing the rules failed.
    #[error(transparent)]
    Rules(#[from] StoreError),

    /// The transactions or rules changed since the preview.
    #[error("the transactions or rules changed since the preview; preview the apply again")]
    Stale,

    /// The apply would exclude rows (or change their exclusion) on or
    /// before their account's `reconciled_through`.
    #[error(
        "{} transaction(s) would change whether they count on or before their account's \
         locked date; unlock the account first",
        .0.len()
    )]
    Locked(Vec<TransactionId>),

    /// A transaction to include isn't one with a change that needs
    /// inclusion.
    #[error("transaction {0} has no value set by the user that this apply would change")]
    NotIncludable(TransactionId),

    /// The database refused the operation.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Plans applying rule `rule` to every stored transaction it matches, with
/// the current rule set ([`rule`](crate::rule) module docs, "Apply").
/// Reads only.
pub fn preview_apply(db: &Database, rule: RuleId) -> Result<ApplyPreview, ApplyError> {
    // One read transaction, so every query sees the same database.
    let tx = Transaction::new_unchecked(db.conn(), TransactionBehavior::Deferred)?;
    let preview = plan_history(&tx, rule)?;
    drop(tx);
    Ok(preview)
}

/// Applies `preview` as one `rule_apply` change batch, in one DB
/// transaction: every change that needs no inclusion, and on the rows in
/// `included` the ones that overwrite a value the user set.
///
/// Plans again first and refuses with [`ApplyError::Stale`] when anything
/// differs from the preview. Refuses [`ApplyError::NotIncludable`] for an
/// included row with nothing to include, and [`ApplyError::Locked`] while a
/// change it would make meets a lock. Undo with [`change::undo`].
pub fn commit_apply(
    db: &mut Database,
    preview: &ApplyPreview,
    included: &BTreeSet<TransactionId>,
) -> Result<Applied, ApplyError> {
    let tx = db
        .conn_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let applied = commit_apply_in(&tx, preview, included)?;
    tx.commit()?;
    tracing::info!(
        rule = %preview.rule,
        transactions = applied.transactions,
        items = applied.items,
        included = included.len(),
        "applied a rule to past transactions"
    );
    Ok(applied)
}

/// [`commit_apply`] inside the caller's DB transaction.
pub(crate) fn commit_apply_in(
    tx: &Connection,
    preview: &ApplyPreview,
    included: &BTreeSet<TransactionId>,
) -> Result<Applied, ApplyError> {
    if let Some(id) = included.iter().find(|id| {
        !preview
            .rows
            .iter()
            .any(|row| row.transaction == **id && row.needs_inclusion())
    }) {
        return Err(ApplyError::NotIncludable(*id));
    }
    if plan_history(tx, preview.rule)? != *preview {
        return Err(ApplyError::Stale);
    }
    let locked: Vec<TransactionId> = preview
        .rows
        .iter()
        .filter(|row| row.locked_if(included.contains(&row.transaction)))
        .map(|row| row.transaction)
        .collect();
    if !locked.is_empty() {
        return Err(ApplyError::Locked(locked));
    }

    let mut writer = Writer::default();
    let mut recorder = Recorder::new(ChangeKind::RuleApply);
    for row in &preview.rows {
        let include = included.contains(&row.transaction);
        writer.write(tx, row.transaction, &row.changes, include, &mut recorder)?;
    }
    let transactions = recorder.transactions();
    let items = recorder.changes().count();
    let batch = recorder.finish(tx).map_err(StoreError::from)?;
    Ok(Applied {
        batch,
        transactions,
        items,
    })
}

/// [`preview_apply`] inside the caller's DB transaction.
pub(crate) fn preview_apply_in(
    conn: &Connection,
    rule: RuleId,
) -> Result<ApplyPreview, ApplyError> {
    plan_history(conn, rule)
}

/// The plan for applying `rule` to every transaction, inside the caller's
/// DB transaction.
fn plan_history(conn: &Connection, rule: RuleId) -> Result<ApplyPreview, ApplyError> {
    let rules = RuleSet::load(conn)?;
    if !rules.contains(rule) {
        return Err(StoreError::NotFound(rule).into());
    }
    let mut preview = ApplyPreview {
        rule,
        rows: Vec::new(),
        split_rows: Vec::new(),
        skipped_splits: Vec::new(),
    };
    let mut planner = Planner::new(conn);
    for row in load_candidates(conn, "1", [])? {
        let Some(outcome) = rules.evaluate_rule(rule, &row.input()) else {
            continue;
        };
        if row.split {
            preview.split_rows.push(row.id);
            continue;
        }
        if outcome.is_empty() {
            continue;
        }
        let (changes, skipped) = planner.plan_row(&row, &outcome)?;
        preview.skipped_splits.extend(skipped);
        if !changes.is_empty() {
            preview.rows.push(RowPlan {
                transaction: row.id,
                changes,
            });
        }
    }
    Ok(preview)
}
