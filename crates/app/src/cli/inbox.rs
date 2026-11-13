//! `coffer inbox` (ROADMAP M2.7): review the suggestions AI clients (and
//! Coffer itself) filed, and approve or reject them, until the app has an
//! Inbox screen ([`coffer_core::inbox`] documents what each does).
//!
//! - `coffer inbox [list]`: the pending suggestions, one line each: ID,
//!   status, kind, source, date filed and a summary of IDs, category keys
//!   and amounts. `--all`, `--status` and `--kind` choose others.
//! - `coffer inbox show <ID>`: one suggestion in full and, while it is
//!   pending, what approving it would change now ([`inbox::impact`]).
//! - `coffer inbox approve <ID>... [--apply-to-history]`: applies each and
//!   marks it approved. `--apply-to-history` is for rules only.
//! - `coffer inbox reject <ID>... [--reason <TEXT>]`: marks each rejected.
//!
//! Approving here is the user's action: the IDs are named on the command
//! line, and nothing is ever asked on stdin. Every ID is checked to exist
//! and be pending before anything is decided, so a typo or a decided ID
//! writes nothing. Each approval is then its own DB transaction.
//!
//! **Untrusted text.** Rationales, rule patterns, merchant names and tag
//! names in a payload are an AI client's words, or (for Coffer's own
//! suggestions) taken from transaction text. They are printed on one line
//! each, labelled with where they came from, stripped of control,
//! zero-width and bidi characters (saying how many were removed), and cut
//! to a length. The summary in `list` leaves them out altogether. None of
//! it decides anything. Saved reports aren't shown here yet.

use std::collections::BTreeSet;
use std::io::Write;

use anyhow::{Context, bail};
use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Args, Subcommand};
use coffer_core::db::Database;
use coffer_core::inbox::payload::CategorizationItem;
use coffer_core::inbox::{
    self, Applied, ApproveOptions, Approved, COFFER_SOURCE, Filter, Payload, Suggestion,
    SuggestionId, SuggestionKind, SuggestionStatus,
};
use coffer_core::rule::{RuleAction, RuleDefinition};

use super::import::plural;
use super::write_table;

#[derive(Debug, Args)]
pub struct InboxArgs {
    /// What to do; lists the pending suggestions when left out.
    #[command(subcommand)]
    pub command: Option<InboxCommand>,
}

#[derive(Debug, Subcommand)]
pub enum InboxCommand {
    /// List suggestions: the pending ones unless asked otherwise (the
    /// default).
    List(ListArgs),
    /// Show a suggestion in full, and what approving it would change now.
    Show(ShowArgs),
    /// Approve suggestions: apply each one and mark it approved.
    Approve(ApproveArgs),
    /// Reject suggestions: mark each one rejected. Nothing else changes.
    Reject(RejectArgs),
}

#[derive(Debug, Default, Args)]
pub struct ListArgs {
    /// List every suggestion, whatever its status.
    #[arg(long, conflicts_with = "status")]
    pub all: bool,

    /// Only suggestions with this status (default: pending).
    #[arg(long, value_parser = status_parser())]
    pub status: Option<SuggestionStatus>,

    /// Only suggestions of this kind.
    #[arg(long, value_parser = kind_parser())]
    pub kind: Option<SuggestionKind>,
}

#[derive(Debug, Args)]
pub struct ShowArgs {
    /// The suggestion's ID, as `coffer inbox` lists it.
    #[arg(value_name = "ID")]
    pub id: i64,
}

#[derive(Debug, Args)]
pub struct ApproveArgs {
    /// The suggestions' IDs, as `coffer inbox` lists them.
    #[arg(value_name = "ID", required = true)]
    pub ids: Vec<i64>,

    /// For a rule: also apply it to past transactions (only those with no
    /// category or a rule's, never one you set). Other kinds ignore it.
    #[arg(long)]
    pub apply_to_history: bool,
}

#[derive(Debug, Args)]
pub struct RejectArgs {
    /// The suggestions' IDs, as `coffer inbox` lists them.
    #[arg(value_name = "ID", required = true)]
    pub ids: Vec<i64>,

    /// Why, kept with each rejected suggestion (at most 500 characters).
    #[arg(long, value_name = "TEXT")]
    pub reason: Option<String>,
}

fn status_parser() -> impl TypedValueParser<Value = SuggestionStatus> {
    PossibleValuesParser::new(SuggestionStatus::ALL.iter().map(|status| status.as_str())).map(
        |key| {
            key.parse()
                .expect("the parser only accepts the statuses' keys")
        },
    )
}

fn kind_parser() -> impl TypedValueParser<Value = SuggestionKind> {
    PossibleValuesParser::new(SuggestionKind::ALL.iter().map(|kind| kind.as_str())).map(|key| {
        key.parse()
            .expect("the parser only accepts the kinds' keys")
    })
}

/// `coffer inbox`.
pub fn run(db: &mut Database, args: InboxArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    match args.command {
        None => list(db, &ListArgs::default(), out),
        Some(InboxCommand::List(args)) => list(db, &args, out),
        Some(InboxCommand::Show(args)) => show(db, &args, out),
        Some(InboxCommand::Approve(args)) => approve(db, &args, out),
        Some(InboxCommand::Reject(args)) => reject(db, &args, out),
    }
}

// Listing -----------------------------------------------------------------------

/// `coffer inbox list`.
pub fn list(db: &Database, args: &ListArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let filter = Filter {
        status: if args.all {
            None
        } else {
            Some(args.status.unwrap_or(SuggestionStatus::Pending))
        },
        kind: args.kind,
    };
    let suggestions = inbox::list(db, &filter).context("couldn't read the inbox")?;
    if suggestions.is_empty() {
        let status = filter.status.map(|s| format!("{s} ")).unwrap_or_default();
        let kind = filter.kind.map(|k| format!("{k} ")).unwrap_or_default();
        writeln!(out, "No {status}{kind}suggestions.")?;
        return Ok(());
    }
    let rows: Vec<Vec<String>> = suggestions
        .iter()
        .map(|s| {
            vec![
                s.id.to_string(),
                s.status.to_string(),
                s.kind.to_string(),
                source_name(&s.source_client),
                s.created_at.chars().take(10).collect(),
                summary(&s.payload),
            ]
        })
        .collect();
    write_table(
        out,
        &["ID", "STATUS", "KIND", "FROM", "FILED", "PROPOSES"],
        &rows,
    )?;
    if suggestions
        .iter()
        .any(|s| s.status == SuggestionStatus::Pending)
    {
        writeln!(out)?;
        writeln!(
            out,
            "`coffer inbox show <ID>` shows one in full and what approving it changes; \
             `coffer inbox approve <ID>` or `coffer inbox reject <ID>` decides it."
        )?;
    }
    Ok(())
}

/// The payload in a line of IDs, category keys and amounts: no text from
/// the client or the bank (`show` has that, labelled).
fn summary(payload: &Payload) -> String {
    match payload {
        Payload::Categorization(items) => {
            const SHOWN: usize = 3;
            let mut text: Vec<String> = items.iter().take(SHOWN).map(item).collect();
            if items.len() > SHOWN {
                text.push(format!("and {} more", items.len() - SHOWN));
            }
            text.join(", ")
        }
        Payload::Rule(rule) => {
            let actions: Vec<String> = rule.actions().iter().map(action_summary).collect();
            format!(
                "rule on {} ({}) -> {}",
                rule.field(),
                rule.pattern().kind(),
                actions.join(", ")
            )
        }
        Payload::TransferPair(pair) => format!(
            "transfer: transaction {} out, transaction {} in",
            pair.outflow, pair.inflow
        ),
        Payload::MerchantAlias(alias) => {
            format!("merchant name for a pattern ({} match)", alias.kind())
        }
        Payload::RefundLink(link) => {
            let amount = link.amount_minor.map_or_else(
                || "as much as fits".to_owned(),
                |minor| format!("{minor} minor units"),
            );
            format!(
                "{}: transaction {} pays back transaction {}, {amount}",
                link.kind.as_str(),
                link.refund,
                link.purchase
            )
        }
    }
}

fn item(item: &CategorizationItem) -> String {
    format!("{} -> {}", item.target, item.category)
}

/// An action without its text: kind, category keys, counts.
fn action_summary(action: &RuleAction) -> String {
    match action {
        RuleAction::SetCategory(key) => format!("set_category {key}"),
        RuleAction::SplitTemplate(template) => {
            let lines: Vec<String> = template
                .lines()
                .iter()
                .map(|line| {
                    format!(
                        "{} {}",
                        line.category(),
                        percent(line.share().basis_points())
                    )
                })
                .collect();
            format!("split_template {}", lines.join(" / "))
        }
        RuleAction::AddTag(tags) => format!("add_tag ({})", plural(tags.tags().len(), "tag")),
        other => other.kind().to_string(),
    }
}

/// Basis points as a percentage: `6000` is `60%`, `3333` is `33.33%`.
fn percent(basis_points: u16) -> String {
    let (whole, part) = (basis_points / 100, basis_points % 100);
    if part == 0 {
        format!("{whole}%")
    } else {
        format!("{whole}.{part:02}%")
    }
}

// Showing -----------------------------------------------------------------------

/// The longest rationale shown, in characters.
const RATIONALE_SHOWN: usize = 500;

/// The longest pattern, name or tag shown, in characters.
const TEXT_SHOWN: usize = 200;

/// `coffer inbox show`.
pub fn show(db: &Database, args: &ShowArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let s = inbox::get(db, SuggestionId::new(args.id))?;
    let from = source_name(&s.source_client);
    writeln!(out, "Suggestion {}: {}, {}", s.id, s.kind, s.status)?;
    writeln!(out, "From {from}, filed {}", s.created_at)?;
    writeln!(out, "Proposes:")?;
    write_payload(out, &s)?;
    if let Some(rationale) = &s.rationale {
        writeln!(
            out,
            "{}",
            labelled("Rationale", &from, rationale, RATIONALE_SHOWN)
        )?;
    }

    if s.status == SuggestionStatus::Pending {
        match inbox::impact(db, s.id) {
            Ok(impact) => writeln!(out, "Approving it now: {impact}.")?,
            Err(error) => writeln!(out, "Approving it now would be refused: {error}.")?,
        }
        if s.kind == SuggestionKind::Rule {
            writeln!(
                out,
                "Approving stores the rule for new imports. With --apply-to-history it is also \
                 applied to past transactions: only those with no category or a rule's, never \
                 one you set."
            )?;
        }
        return Ok(());
    }

    writeln!(
        out,
        "{} {}",
        if s.status == SuggestionStatus::Approved {
            "Approved"
        } else {
            "Rejected"
        },
        s.decided_at.as_deref().unwrap_or("(no date recorded)")
    )?;
    if let Some(reason) = &s.decision_reason {
        let shown = untrusted(reason, TEXT_SHOWN);
        writeln!(out, "Your reason: \"{}\"{}", shown.text, shown.notes())?;
    }
    if let Some(batch) = s.change_batch {
        writeln!(out, "Change batch: {batch}")?;
    }
    if let Some(applied) = &s.applied {
        // Coffer's own record of what it made, by ID.
        writeln!(out, "Made: {applied}")?;
    }
    Ok(())
}

fn write_payload(out: &mut dyn Write, s: &Suggestion) -> std::io::Result<()> {
    let from = source_name(&s.source_client);
    match &s.payload {
        Payload::Categorization(items) => {
            for entry in items {
                writeln!(out, "  {}", item(entry))?;
            }
        }
        Payload::Rule(rule) => write_rule(out, rule, &from)?,
        Payload::TransferPair(pair) => {
            writeln!(out, "  transaction {} (money out)", pair.outflow)?;
            writeln!(out, "  transaction {} (the same amount in)", pair.inflow)?;
            writeln!(out, "  as one transfer between two accounts")?;
        }
        Payload::MerchantAlias(alias) => {
            writeln!(out, "  a merchant name, matching {}", alias.kind())?;
            writeln!(
                out,
                "  {}",
                labelled("Pattern", &from, alias.pattern(), TEXT_SHOWN)
            )?;
            writeln!(
                out,
                "  {}",
                labelled("Name", &from, alias.name().as_str(), TEXT_SHOWN)
            )?;
        }
        Payload::RefundLink(link) => {
            writeln!(
                out,
                "  transaction {} (money in) as a {} of transaction {} (the purchase)",
                link.refund,
                link.kind.as_str(),
                link.purchase
            )?;
            match link.amount_minor {
                Some(minor) => writeln!(out, "  amount: {minor} (minor units)")?,
                None => writeln!(out, "  amount: as much as both sides can take")?,
            }
        }
    }
    Ok(())
}

fn write_rule(out: &mut dyn Write, rule: &RuleDefinition, from: &str) -> std::io::Result<()> {
    writeln!(
        out,
        "  a rule matching {} ({})",
        rule.field(),
        rule.pattern().kind()
    )?;
    writeln!(
        out,
        "  {}",
        labelled("Pattern", from, rule.pattern().as_str(), TEXT_SHOWN)
    )?;
    let amount = rule.amount();
    match (amount.min(), amount.max()) {
        (None, None) => {}
        (min, max) => writeln!(
            out,
            "  amount (minor units): {} to {}",
            min.map_or_else(|| "any".to_owned(), |m| m.to_string()),
            max.map_or_else(|| "any".to_owned(), |m| m.to_string())
        )?,
    }
    if let Some(account) = rule.account_id() {
        writeln!(out, "  only on account {account}")?;
    }
    for action in rule.actions() {
        match action {
            RuleAction::SetMerchant(name) => writeln!(
                out,
                "  then set_merchant: {}",
                labelled("name", from, name.as_str(), TEXT_SHOWN)
            )?,
            RuleAction::AddTag(tags) => {
                writeln!(out, "  then add_tag:")?;
                for tag in tags.tags() {
                    writeln!(
                        out,
                        "    {}",
                        labelled("tag", from, tag.as_str(), TEXT_SHOWN)
                    )?;
                }
            }
            other => writeln!(out, "  then {}", action_summary(other))?,
        }
    }
    Ok(())
}

/// Who filed a suggestion, for labels: the client's name, or Coffer.
fn source_name(source: &str) -> String {
    if source == COFFER_SOURCE {
        "Coffer".to_owned()
    } else {
        // A checked client name when it was filed; stripped all the same.
        untrusted(source, 64).text
    }
}

// Deciding ----------------------------------------------------------------------

/// `coffer inbox approve`.
pub fn approve(db: &mut Database, args: &ApproveArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let suggestions = all_pending(db, &args.ids, "approved")?;
    if args.apply_to_history && !suggestions.iter().any(|s| s.kind == SuggestionKind::Rule) {
        writeln!(
            out,
            "Note: --apply-to-history is for rules; none of these is a rule, so it changes \
             nothing."
        )?;
    }
    let options = ApproveOptions {
        apply_to_history: args.apply_to_history,
    };
    let mut done: Vec<SuggestionId> = Vec::new();
    let mut settled: BTreeSet<SuggestionId> = BTreeSet::new();
    for (index, s) in suggestions.iter().enumerate() {
        if settled.contains(&s.id) {
            writeln!(
                out,
                "Suggestion {} ({}) was settled by the rule approved above: it is approved.",
                s.id, s.kind
            )?;
            continue;
        }
        match inbox::approve(db, s.id, options) {
            Ok(approved) => {
                write_approved(out, &approved)?;
                settled.extend(approved.settled.iter().copied());
                done.push(s.id);
            }
            Err(error) => {
                let left: Vec<SuggestionId> = suggestions[index..].iter().map(|s| s.id).collect();
                bail!(
                    "couldn't approve suggestion {}: {error}{}",
                    s.id,
                    partial("approved", &done, &left)
                );
            }
        }
    }
    Ok(())
}

fn write_approved(out: &mut dyn Write, approved: &Approved) -> std::io::Result<()> {
    write!(
        out,
        "Approved suggestion {} ({}): ",
        approved.id, approved.kind
    )?;
    match &approved.applied {
        Applied::Categorized {
            batch: Some(batch),
            transactions,
        } => writeln!(
            out,
            "set the category of {} (change batch {batch}).",
            plural(*transactions, "transaction")
        )?,
        Applied::Categorized { batch: None, .. } => writeln!(
            out,
            "nothing needed changing: every target already had that category."
        )?,
        Applied::Rule { rule, history } => {
            write!(out, "stored as rule {rule}, first in the rule list")?;
            match history {
                Some(batch) => writeln!(
                    out,
                    "; applied to past transactions (change batch {batch})."
                )?,
                None => writeln!(out, "; past transactions are unchanged.")?,
            }
        }
        Applied::TransferPair { group } => writeln!(
            out,
            "the two rows are one transfer (transfer group {group})."
        )?,
        Applied::MerchantAlias { alias, renamed } => writeln!(
            out,
            "stored as merchant name {alias}; renamed {}.",
            plural(*renamed, "transaction")
        )?,
        Applied::RefundLink { link } => writeln!(out, "linked (link {link}).")?,
    }
    if !approved.settled.is_empty() {
        writeln!(
            out,
            "  It also settled {}: categorisations applying the rule already made, now approved.",
            ids(&approved.settled, "suggestion")
        )?;
    }
    Ok(())
}

/// `coffer inbox reject`.
pub fn reject(db: &mut Database, args: &RejectArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let suggestions = all_pending(db, &args.ids, "rejected")?;
    let mut done = Vec::new();
    for (index, s) in suggestions.iter().enumerate() {
        if let Err(error) = inbox::reject(db, s.id, args.reason.as_deref()) {
            let left: Vec<SuggestionId> = suggestions[index..].iter().map(|s| s.id).collect();
            bail!(
                "couldn't reject suggestion {}: {error}{}",
                s.id,
                partial("rejected", &done, &left)
            );
        }
        writeln!(out, "Rejected suggestion {} ({}).", s.id, s.kind)?;
        done.push(s.id);
    }
    if args.reason.is_some() {
        writeln!(
            out,
            "Your reason is kept with {}.",
            if done.len() == 1 { "it" } else { "them" }
        )?;
    }
    Ok(())
}

/// The suggestions `ids` names, in order, once each, if every one exists
/// and is pending. Otherwise an error saying nothing was `verb`.
fn all_pending(db: &Database, ids: &[i64], verb: &str) -> anyhow::Result<Vec<Suggestion>> {
    let mut seen = BTreeSet::new();
    let mut suggestions = Vec::with_capacity(ids.len());
    for &id in ids {
        let id = SuggestionId::new(id);
        if !seen.insert(id) {
            bail!("suggestion {id} is named twice; nothing was {verb}");
        }
        let suggestion = inbox::get(db, id).with_context(|| format!("nothing was {verb}"))?;
        if suggestion.status != SuggestionStatus::Pending {
            bail!(
                "suggestion {id} is already {}; nothing was {verb}",
                suggestion.status
            );
        }
        suggestions.push(suggestion);
    }
    Ok(suggestions)
}

/// After a failure part way: what was decided already and what was left.
fn partial(verb: &str, done: &[SuggestionId], left: &[SuggestionId]) -> String {
    let mut text = String::new();
    if !done.is_empty() {
        text.push_str(&format!("; {} {verb} before it", ids(done, "suggestion")));
    }
    text.push_str(&format!("; {} still pending", ids(left, "suggestion")));
    text
}

/// "suggestion 3" or "suggestions 3, 4".
fn ids(ids: &[SuggestionId], noun: &str) -> String {
    let list: Vec<String> = ids.iter().map(ToString::to_string).collect();
    match list.len() {
        1 => format!("{noun} {}", list[0]),
        _ => format!("{noun}s {}", list.join(", ")),
    }
}

// Untrusted text ----------------------------------------------------------------

/// Untrusted text made safe to print on one line.
#[derive(Debug, PartialEq, Eq)]
struct Shown {
    text: String,
    /// Control, zero-width and bidi characters removed.
    hidden: usize,
    /// Characters cut off the end.
    cut: usize,
}

impl Shown {
    /// What was done to the text, to print after it.
    fn notes(&self) -> String {
        let mut notes = String::new();
        if self.cut > 0 {
            notes.push_str(&format!(
                " [and {} not shown]",
                plural(self.cut, "more character")
            ));
        }
        if self.hidden > 0 {
            notes.push_str(&format!(
                " [{} removed: control, invisible or direction characters]",
                plural(self.hidden, "hidden character")
            ));
        }
        notes
    }
}

/// `label (from <who>, unverified): "<text>"` and what was done to it.
fn labelled(label: &str, from: &str, raw: &str, max: usize) -> String {
    let shown = untrusted(raw, max);
    format!(
        "{label} (from {from}, unverified): \"{}\"{}",
        shown.text,
        shown.notes()
    )
}

/// Strips `raw` for a terminal: line breaks, tabs and other whitespace
/// become single spaces; control, zero-width, bidi and other invisible
/// characters are removed and counted; the rest is cut to `max` characters.
/// (`coffer_core::envelope::sanitize` is for text going to an AI client.)
fn untrusted(raw: &str, max: usize) -> Shown {
    let mut text = String::new();
    let mut hidden = 0;
    let mut space = false;
    for c in raw.chars() {
        if c.is_whitespace() {
            space = !text.is_empty();
        } else if c.is_control() || is_invisible(c) {
            hidden += 1;
        } else {
            if space {
                text.push(' ');
                space = false;
            }
            text.push(c);
        }
    }
    let length = text.chars().count();
    let cut = length.saturating_sub(max);
    if cut > 0 {
        text = text.chars().take(max).collect();
    }
    Shown { text, hidden, cut }
}

/// Format characters that print as nothing, or reorder or hide what comes
/// after them: zero-width and joiners, bidi controls and isolates, the BOM,
/// variation selectors, Unicode tags, interlinear annotations, and blank
/// fillers.
fn is_invisible(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD
            | 0x034F
            | 0x061C
            | 0x115F..=0x1160
            | 0x17B4..=0x17B5
            | 0x180B..=0x180F
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x206F
            | 0x3164
            | 0xFE00..=0xFE0F
            | 0xFEFF
            | 0xFFA0
            | 0xFFF0..=0xFFFB
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0000..=0xE0FFF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_controls_invisibles_and_bidi() {
        let shown = untrusted(
            "Approve\u{1b}[31m all\u{202e}evil\u{202c}\u{200b}\n\tnow\u{e0041}\u{feff}",
            100,
        );
        assert_eq!(shown.text, "Approve[31m allevil now");
        assert_eq!(shown.hidden, 6);
        assert_eq!(shown.cut, 0);
        assert!(
            shown.notes().contains("6 hidden characters removed"),
            "{}",
            shown.notes()
        );
    }

    #[test]
    fn cuts_to_the_length_and_trims() {
        let shown = untrusted("  \u{2028}abcdef  ", 4);
        assert_eq!(
            shown,
            Shown {
                text: "abcd".to_owned(),
                hidden: 0,
                cut: 2
            }
        );
        assert_eq!(shown.notes(), " [and 2 more characters not shown]");
        assert_eq!(untrusted("é".repeat(3).as_str(), 3).cut, 0);
    }

    #[test]
    fn percentages() {
        assert_eq!(percent(6000), "60%");
        assert_eq!(percent(3333), "33.33%");
        assert_eq!(percent(5), "0.05%");
    }
}
