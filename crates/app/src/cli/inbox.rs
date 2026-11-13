//! `coffer inbox` (ROADMAP M2.7): review the suggestions AI clients (and
//! Coffer itself) filed, and approve or reject them, until the app has an
//! Inbox screen ([`coffer_core::inbox`] documents what each does).
//!
//! - `coffer inbox [list]`: the pending suggestions, one line each: ID,
//!   status, kind, source, date filed and a summary of IDs, category keys
//!   and amounts. `--all`, `--status` and `--kind` choose others.
//! - `coffer inbox show <ID>`: one suggestion in full and, while it is
//!   pending, what approving it would change now ([`inbox::impact`]).
//! - `coffer inbox approve <ID>... [--apply-to-history]`: applies them all
//!   and marks them approved, or none of them. `--apply-to-history` is for
//!   rules only.
//! - `coffer inbox reject <ID>... [--reason <TEXT>]`: marks each rejected.
//! - `coffer inbox reports`: the saved reports, newest first, one line each:
//!   ID, period, source, date saved and title. No body.
//! - `coffer inbox report <ID>`: one saved report in full, as plain text.
//! - `coffer inbox delete-report <ID>... [--yes]` or
//!   `coffer inbox delete-report --client <NAME> [--before <DATE>] [--yes]`:
//!   deletes saved reports, all or none ([`inbox::delete_reports`]). Without
//!   `--yes` it only lists what would go, like `coffer restore`. This is how
//!   the user makes room for a client at the report cap.
//!
//! Approving here is the user's action: the IDs are named on the command
//! line, and nothing is ever asked on stdin. Every ID is checked to exist
//! and be pending before anything is decided, so a typo or a decided ID
//! writes nothing. The approvals then run in one DB transaction
//! ([`inbox::approve_all`]): if one fails, none is applied. A suggestion
//! that a rule earlier in the list settled is reported as settled, not
//! refused.
//!
//! **Untrusted text.** Rationales, rule patterns, merchant names and tag
//! names in a payload are an AI client's words, or (for Coffer's own
//! suggestions) taken from transaction text; a saved report's period, title
//! and body are an AI client's words. They are printed labelled with where
//! they came from, stripped of the characters the MCP envelope strips
//! ([`envelope::is_invisible`]: control, zero-width, bidi and other
//! invisible characters), saying how many were removed, and cut to a
//! length. Each is on one line, except a report's body, which keeps its
//! line breaks with every line indented under a bar, so it can't pass for
//! Coffer's own output. A body is printed as written, markdown and all:
//! nothing is rendered, no link is followed, nothing is fetched. The
//! summary in `list` leaves free text out altogether, and `reports` and
//! `delete-report` show only the title and period. None of it decides
//! anything.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use anyhow::{Context, bail};
use chrono::NaiveDate;
use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Args, Subcommand};
use coffer_core::db::Database;
use coffer_core::envelope;
use coffer_core::inbox::payload::CategorizationItem;
use coffer_core::inbox::{
    self, Applied, ApproveOptions, Approved, COFFER_SOURCE, Filter, Payload, ReportHeading,
    ReportSelection, Suggestion, SuggestionId, SuggestionKind, SuggestionStatus,
};
use coffer_core::mcp_client::ClientName;
use coffer_core::reporting::Limit;
use coffer_core::reporting::catalog::{self, SavedReport};
use coffer_core::rule::{RuleAction, RuleDefinition};

use super::import::plural;
use super::schedules::parse_date;
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
    /// Approve suggestions: apply them and mark them approved, all or
    /// none.
    Approve(ApproveArgs),
    /// Reject suggestions: mark each one rejected. Nothing else changes.
    Reject(RejectArgs),
    /// List the reports AI clients saved, newest first (titles only).
    Reports,
    /// Show a saved report in full, as plain text.
    Report(ReportArgs),
    /// Delete saved reports, by ID or by client: all or none. Without
    /// --yes, only shows what would be deleted.
    DeleteReport(DeleteReportArgs),
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

#[derive(Debug, Args)]
pub struct ReportArgs {
    /// The report's ID, as `coffer inbox reports` lists it.
    #[arg(value_name = "ID")]
    pub id: i64,
}

#[derive(Debug, Args)]
pub struct DeleteReportArgs {
    /// The reports' IDs, as `coffer inbox reports` lists them.
    #[arg(
        value_name = "ID",
        required_unless_present = "client",
        conflicts_with = "client"
    )]
    pub ids: Vec<i64>,

    /// Every report this client saved, instead of IDs.
    #[arg(long, value_name = "NAME")]
    pub client: Option<ClientName>,

    /// With --client: only the reports saved before this day (YYYY-MM-DD,
    /// UTC).
    #[arg(
        long,
        value_name = "DATE",
        requires = "client",
        conflicts_with = "ids",
        value_parser = parse_date
    )]
    pub before: Option<NaiveDate>,

    /// Delete them. Without this, only shows what deleting would remove.
    #[arg(long, short = 'y')]
    pub yes: bool,
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
        Some(InboxCommand::Reports) => reports(db, out),
        Some(InboxCommand::Report(args)) => report(db, &args, out),
        Some(InboxCommand::DeleteReport(args)) => delete_report(db, &args, out),
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
    let ids: Vec<SuggestionId> = suggestions.iter().map(|s| s.id).collect();
    let approved = match inbox::approve_all(db, &ids, options) {
        Ok(approved) => approved,
        Err(error) => bail!("{error}; nothing was approved"),
    };
    let mut approved: BTreeMap<SuggestionId, Approved> =
        approved.into_iter().map(|a| (a.id, a)).collect();
    for s in &suggestions {
        match approved.remove(&s.id) {
            Some(one) => write_approved(out, &one)?,
            // `approve_all` skips only what an earlier rule settled.
            None => writeln!(
                out,
                "Suggestion {} ({}) was settled by the rule approved above: it is approved.",
                s.id, s.kind
            )?,
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

// Saved reports -----------------------------------------------------------------

/// The longest report period shown, in characters (the longest stored).
const PERIOD_SHOWN: usize = inbox::MAX_REPORT_PERIOD_CHARS;

/// The longest report body shown, in characters: the longest stored, so a
/// body is cut only if it was stored some other way.
const BODY_SHOWN: usize = inbox::MAX_REPORT_BODY_CHARS;

/// `coffer inbox reports`.
pub fn reports(db: &Database, out: &mut dyn Write) -> anyhow::Result<()> {
    let reader = db.reader().context("couldn't read the reports")?;
    let saved = catalog::reports(&reader, Limit::MAX).context("couldn't read the reports")?;
    if saved.is_empty() {
        writeln!(out, "No saved reports.")?;
        return Ok(());
    }
    let rows: Vec<Vec<String>> = saved
        .items
        .iter()
        .map(|r| report_row(r.id, &r.period, &r.source_client, &r.created_at, &r.title))
        .collect();
    write_report_table(out, &rows)?;
    writeln!(out)?;
    if saved.truncated {
        writeln!(out, "Only the newest {} are listed.", saved.len())?;
    }
    writeln!(
        out,
        "Periods and titles are the AI clients' own words, unverified. \
         `coffer inbox report <ID>` shows one in full; \
         `coffer inbox delete-report <ID>` deletes it."
    )?;
    let counts = inbox::report_counts(db).context("couldn't count the reports")?;
    for (source, count) in counts {
        if count >= inbox::MAX_REPORTS_PER_SOURCE {
            writeln!(
                out,
                "{} has saved {count} reports, the most one client may keep, and can't save \
                 more until you delete some: `coffer inbox delete-report --client {}` deletes \
                 them (with --before <DATE>, only older ones).",
                source_name(&source),
                untrusted(&source, 64).text
            )?;
        }
    }
    Ok(())
}

/// One line of a report table: ID, period, source, day saved and title,
/// the client's words stripped and labelled as untrusted text.
fn report_row(id: i64, period: &str, source: &str, created_at: &str, title: &str) -> Vec<String> {
    let period = untrusted(period, PERIOD_SHOWN);
    let title = untrusted(title, TEXT_SHOWN);
    vec![
        id.to_string(),
        format!("{}{}", period.text, period.notes()),
        source_name(source),
        created_at.chars().take(10).collect(),
        format!("\"{}\"{}", title.text, title.notes()),
    ]
}

fn write_report_table(out: &mut dyn Write, rows: &[Vec<String>]) -> std::io::Result<()> {
    write_table(out, &["ID", "PERIOD", "FROM", "SAVED", "TITLE"], rows)
}

/// `coffer inbox report`.
pub fn report(db: &Database, args: &ReportArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let reader = db.reader().context("couldn't read the report")?;
    let Some(report) = catalog::report(&reader, args.id).context("couldn't read the report")?
    else {
        bail!("there is no report {}", args.id);
    };
    write_report(out, &report)?;
    Ok(())
}

/// `coffer inbox delete-report`. Without `--yes` it lists what would be
/// deleted and changes nothing.
pub fn delete_report(
    db: &mut Database,
    args: &DeleteReportArgs,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let selection = match &args.client {
        Some(source) => ReportSelection::Source {
            source: source.clone(),
            before: args.before,
        },
        None => ReportSelection::Ids(args.ids.clone()),
    };
    let reports = if args.yes {
        inbox::delete_reports(db, &selection)
    } else {
        inbox::reports_to_delete(db, &selection)
    }
    .context("nothing was deleted")?;

    if reports.is_empty() {
        // Only a client's reports can be none.
        let source = args.client.as_ref().map_or_else(String::new, |c| {
            format!(" from {}", source_name(c.as_str()))
        });
        let before = args
            .before
            .map_or_else(String::new, |day| format!(" saved before {day}"));
        writeln!(
            out,
            "No saved reports{source}{before}; nothing was deleted."
        )?;
        return Ok(());
    }

    let count = plural(reports.len(), "saved report");
    if args.yes {
        writeln!(out, "Deleted {count}:")?;
    } else {
        writeln!(out, "Deleting would remove {count}:")?;
    }
    write_headings(out, &reports)?;
    writeln!(out)?;
    writeln!(
        out,
        "Periods and titles are the AI clients' own words, unverified."
    )?;
    if !args.yes {
        writeln!(
            out,
            "Nothing was deleted. Run the command again with --yes to delete {}; it can't be \
             undone.",
            if reports.len() == 1 { "it" } else { "them" }
        )?;
    }
    Ok(())
}

fn write_headings(out: &mut dyn Write, reports: &[ReportHeading]) -> std::io::Result<()> {
    let rows: Vec<Vec<String>> = reports
        .iter()
        .map(|r| report_row(r.id, &r.period, &r.source_client, &r.created_at, &r.title))
        .collect();
    write_report_table(out, &rows)
}

fn write_report(out: &mut dyn Write, report: &SavedReport) -> std::io::Result<()> {
    let from = source_name(&report.source_client);
    writeln!(
        out,
        "Report {}: saved by {from}, {}",
        report.id, report.created_at
    )?;
    writeln!(
        out,
        "{}",
        labelled("Period", &from, &report.period, PERIOD_SHOWN)
    )?;
    writeln!(
        out,
        "{}",
        labelled("Title", &from, &report.title, TEXT_SHOWN)
    )?;
    writeln!(
        out,
        "Body (written by {from}, an AI client; unverified, shown as plain text):"
    )?;
    let (lines, shown) = untrusted_lines(&report.body_markdown, BODY_SHOWN);
    if lines.is_empty() {
        writeln!(out, "  | (empty)")?;
    }
    for line in &lines {
        if line.is_empty() {
            writeln!(out, "  |")?;
        } else {
            writeln!(out, "  | {line}")?;
        }
    }
    writeln!(out, "End of report {}.{}", report.id, shown.notes())
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
/// become single spaces (trimmed at the ends); the characters the MCP
/// envelope strips ([`envelope::is_invisible`]: every control character
/// but whitespace, zero-width, bidi and other invisible characters) are
/// removed and counted; the rest is cut to `max` characters. Whitespace is
/// tested first, as the envelope's own clean-up does, so a line separator
/// between two words is a space. Unlike `envelope::sanitize` (for text
/// going to an AI client) nothing is withheld or rewritten: the user reads
/// what the client wrote.
fn untrusted(raw: &str, max: usize) -> Shown {
    let mut text = String::new();
    let mut hidden = 0;
    let mut space = false;
    for c in raw.chars() {
        if c.is_whitespace() {
            space = !text.is_empty();
        } else if envelope::is_invisible(c) {
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

/// Untrusted text that keeps its line breaks: each line of `raw` stripped
/// as [`untrusted`] strips it, runs of blank lines kept as one, blank lines
/// at the ends dropped, and `max` characters shown in all (line breaks not
/// counted).
fn untrusted_lines(raw: &str, max: usize) -> (Vec<String>, Shown) {
    let mut lines: Vec<String> = Vec::new();
    let mut total = Shown {
        text: String::new(),
        hidden: 0,
        cut: 0,
    };
    let mut left = max;
    for line in raw.split('\n') {
        let shown = untrusted(line, left);
        total.hidden += shown.hidden;
        total.cut += shown.cut;
        left -= shown.text.chars().count();
        let blank = shown.text.is_empty();
        if !(blank && lines.last().is_none_or(String::is_empty)) {
            lines.push(shown.text);
        }
    }
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    (lines, total)
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
    fn strips_exactly_what_the_envelope_strips() {
        // The first three planes and the tag and variation-selector
        // block, without whitespace.
        let raw: String = (0u32..0x3_0000)
            .chain(0xE_0000..0xE_1000)
            .filter_map(char::from_u32)
            .filter(|c| !c.is_whitespace())
            .collect();
        let stripped = envelope::strip_invisible(&raw);
        let shown = untrusted(&raw, usize::MAX);
        assert_eq!(shown.text, stripped);
        assert_eq!(shown.hidden, raw.chars().count() - stripped.chars().count());
        // Whitespace the envelope would strip (a carriage return, the line
        // and paragraph separators) is a space here, as in its clean-up.
        let shown = untrusted("a\r\u{2028}b\u{2029}c", 10);
        assert_eq!((shown.text.as_str(), shown.hidden), ("a b c", 0));
    }

    #[test]
    fn lines_keep_their_breaks_and_share_the_limit() {
        let (lines, shown) =
            untrusted_lines("\n# Title\u{200b}\r\n\n\n\nSome\ttext\n- item\n\n", 100);
        assert_eq!(lines, ["# Title", "", "Some text", "- item"]);
        assert_eq!((shown.hidden, shown.cut), (1, 0));

        let (lines, shown) = untrusted_lines("abcd\nefgh\nijkl", 6);
        assert_eq!(lines, ["abcd", "ef"]);
        assert_eq!(shown.cut, 6);
    }

    #[test]
    fn percentages() {
        assert_eq!(percent(6000), "60%");
        assert_eq!(percent(3333), "33.33%");
        assert_eq!(percent(5), "0.05%");
    }
}
