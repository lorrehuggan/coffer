//! What the Import dialog says (M3.4): captions, the preview's notes, the
//! Review step's figures and reconciliation block. Pure, so it's tested
//! without a window.
//!
//! Copy follows Paper (`5ZQ-0`, `6J4-0`) where drawn; the composed states'
//! wording is recorded in `docs/DESIGN.md` → "Import" → "Built in M3.4".
//! Text from the file (a merchant, a flagged value) is the user's own data,
//! shown in their window and never logged.

use chrono::NaiveDate;
use coffer_core::balance::CheckResult;
use coffer_core::import::{
    BalanceCheck, CheckedBalance, Flag, ImportPreview, ReconciliationStatus, RowOutcome,
};
use coffer_core::money::{DecimalMark, Money};
use coffer_core::statement::{RowProblem, StatementLine};
use coffer_import::csv::{Delimiter, TextEncoding};

use crate::format::{Sign, list_date, money};

use super::session::Chosen;

/// A piece of a sentence: plain, or in Geist Mono (a date, an amount, a
/// count, the file's own text).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seg {
    pub text: String,
    pub mono: bool,
}

fn plain(text: impl Into<String>) -> Seg {
    Seg {
        text: text.into(),
        mono: false,
    }
}

fn mono(text: impl Into<String>) -> Seg {
    Seg {
        text: text.into(),
        mono: true,
    }
}

/// The segments as one string, for tests and accessible names.
pub fn joined(segs: &[Seg]) -> String {
    segs.iter().map(|seg| seg.text.as_str()).collect()
}

/// `noun`, plural when `n` isn't one.
fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        noun.to_owned()
    } else {
        format!("{noun}s")
    }
}

/// A count as the app writes it: `1,906`.
pub fn count(n: usize) -> String {
    crate::settings::table::count(n as u64)
}

/// A date or time format as people write it: `%d/%m/%Y` → `DD/MM/YYYY`,
/// `%H:%M:%S` → `HH:mm:ss`.
pub fn pattern_label(pattern: &str) -> String {
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let unpadded = chars.peek() == Some(&'-');
        if unpadded {
            chars.next();
        }
        let token = match (chars.next(), unpadded) {
            (Some('d'), false) => "DD",
            (Some('d'), true) => "D",
            (Some('m'), false) => "MM",
            (Some('m'), true) => "M",
            (Some('Y'), _) => "YYYY",
            (Some('y'), _) => "YY",
            (Some('b'), _) => "MMM",
            (Some('B'), _) => "MMMM",
            (Some('H'), false) => "HH",
            (Some('H'), true) => "H",
            (Some('I'), _) => "hh",
            (Some('M'), _) => "mm",
            (Some('S'), _) => "ss",
            (Some('p'), _) => "AM",
            (Some('%'), _) => "%",
            (Some(other), _) => {
                out.push('%');
                out.push(other);
                continue;
            }
            (None, _) => "%",
        };
        out.push_str(token);
    }
    out
}

/// `comma-separated`.
pub fn delimiter_name(delimiter: Delimiter) -> &'static str {
    match delimiter {
        Delimiter::Comma => "comma-separated",
        Delimiter::Semicolon => "semicolon-separated",
        Delimiter::Tab => "tab-separated",
        Delimiter::Pipe => "pipe-separated",
    }
}

/// The Map step's subtitle: `CSV · UTF-8 · comma-separated · 312 rows`.
pub fn csv_caption(encoding: TextEncoding, delimiter: Delimiter, rows: usize) -> Vec<Seg> {
    vec![
        plain(format!(
            "CSV · {} · {} · ",
            encoding.name(),
            delimiter_name(delimiter)
        )),
        mono(count(rows)),
        plain(format!(" {}", plural(rows, "row"))),
    ]
}

/// The Map step's subtitle for OFX: `OFX · 2 statements`.
pub fn ofx_caption(statements: usize) -> Vec<Seg> {
    vec![
        plain("OFX · "),
        mono(count(statements)),
        plain(format!(" {}", plural(statements, "statement"))),
    ]
}

/// The caption under the Account select: how the mapping was chosen.
pub fn mapping_caption(chosen: &Chosen, preset: Option<&str>, unchanged: bool) -> String {
    match (chosen, preset) {
        (_, Some(name)) if !unchanged => {
            format!("Changed from {name}. Save it as a preset to use it again.")
        }
        (Chosen::Header, _) => {
            "Detected from the header row. Change any column below; the preview updates as you go."
                .to_owned()
        }
        (Chosen::Tie, _) => "More than one preset has this header row: check this is the right \
                             one. The preview updates as you go."
            .to_owned(),
        (Chosen::Account, _) => "The account's preset. Change any column below; the preview \
                                 updates as you go."
            .to_owned(),
        (Chosen::User, _) => "Change any column below; the preview updates as you go.".to_owned(),
        (Chosen::Nothing, _) => "No preset recognises this file. Choose its columns below; the \
                                 preview updates as you go."
            .to_owned(),
    }
}

/// What a preview row shows in its Merchant column, and whether it's a
/// transfer to or from a pot (muted, with the transfer icon): `To Rainy
/// day`, `From Holiday`.
pub fn row_text(line: &StatementLine) -> (String, bool) {
    if let Some(pot) = &line.sub_account {
        let name = pot.name.as_deref().unwrap_or(&pot.key);
        let direction = if line.amount.is_negative() {
            "To"
        } else {
            "From"
        };
        return (format!("{direction} {name}"), true);
    }
    let text = line
        .merchant
        .clone()
        .filter(|merchant| !merchant.trim().is_empty())
        .unwrap_or_else(|| line.description_raw.clone());
    (text.trim().to_owned(), false)
}

/// A row's amount: `−£6.45`, `+£34.00`.
pub fn row_amount(line: &StatementLine) -> String {
    money(line.amount, Sign::Always)
}

/// A row's date in the preview: `9 Nov`.
pub fn row_date(line: &StatementLine, today: NaiveDate) -> String {
    list_date(line.date, today)
}

/// The note under the preview for a flagged row: `Row 8: “8,99” isn't an
/// amount with a decimal point. It will be imported as flagged, not
/// skipped.`
pub fn flag_note(
    number: usize,
    problem: &RowProblem,
    text: Option<&str>,
    decimal: DecimalMark,
    more: usize,
) -> Vec<Seg> {
    let mut segs = vec![plain("Row "), mono(count(number)), plain(": ")];
    match (problem, text) {
        (RowProblem::Amount { .. }, Some(text)) => {
            segs.push(mono(format!("“{text}”")));
            segs.push(plain(format!(
                " isn’t an amount with a decimal {}.",
                match decimal {
                    DecimalMark::Point => "point",
                    DecimalMark::Comma => "comma",
                }
            )));
        }
        (RowProblem::Unreadable { field, .. } | RowProblem::Missing(field), Some(text)) => {
            segs.push(mono(format!("“{text}”")));
            segs.push(plain(format!(
                " isn’t a {} Coffer can read.",
                field.as_str()
            )));
        }
        _ => segs.push(plain(super::session::sentence(&problem.to_string()))),
    }
    segs.push(plain(" It will be imported as flagged, not skipped."));
    if more > 0 {
        segs.push(plain(" "));
        segs.push(mono(count(more)));
        segs.push(plain(format!(
            " more {} flagged too.",
            if more == 1 { "row is" } else { "rows are" }
        )));
    }
    segs
}

/// `Pot transfers are paired with your Holiday and Rainy day pots
/// automatically.`
pub fn pots_note(names: &[String]) -> Option<String> {
    let list = match names {
        [] => return None,
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    Some(format!(
        "Pot transfers are paired with your {list} {} automatically.",
        if names.len() == 1 { "pot" } else { "pots" }
    ))
}

/// The Review step's subtitle: `Monzo (current account) → Current account ·
/// 1 Nov – 9 Nov 2026`.
pub fn review_caption(
    read_with: &str,
    account: &str,
    span: Option<(NaiveDate, NaiveDate)>,
) -> Vec<Seg> {
    let mut segs = vec![plain(format!("{read_with} → {account}"))];
    if let Some((first, last)) = span {
        segs.push(plain(" · "));
        let first = if first.format("%Y").to_string() == last.format("%Y").to_string() {
            first.format("%-d %b").to_string()
        } else {
            first.format("%-d %b %Y").to_string()
        };
        segs.push(mono(format!("{first} – {}", last.format("%-d %b %Y"))));
    }
    segs
}

/// The first and last dates among the statement's rows.
pub fn span(preview: &ImportPreview) -> Option<(NaiveDate, NaiveDate)> {
    let dates = preview
        .rows()
        .filter_map(|row| row.line.map(|line| line.date));
    dates.fold(None, |span, date| match span {
        None => Some((date, date)),
        Some((first, last)) => Some((first.min(date), last.max(date))),
    })
}

/// Beside the Review figures: `16 rows match transactions you already
/// have, so they are skipped.`
pub fn already_caption(already: usize) -> Option<String> {
    (already > 0).then(|| {
        format!(
            "{} {} transactions you already have, so {} skipped.",
            count(already),
            if already == 1 {
                "row matches"
            } else {
                "rows match"
            },
            if already == 1 { "it is" } else { "they are" }
        )
    })
}

/// The bank's name for "Categorised from Monzo's categories": a preset's
/// name without its parenthesis.
pub fn bank_name(preset: &str) -> &str {
    preset.split(" (").next().unwrap_or(preset).trim()
}

/// How a Review block looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Hairline border, the info icon: nothing to check.
    Neutral,
    /// Hairline border, the check: reconciled.
    Reconciled,
    /// `danger-soft`: a mismatch.
    Mismatch,
    /// `warning-soft`, the lock: rows in a locked period.
    Locked,
}

/// One block on the Review step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub tone: Tone,
    pub title: String,
    pub body: Vec<Seg>,
}

fn day(date: NaiveDate) -> Seg {
    mono(date.format("%-d %b").to_string())
}

fn amount(money_value: Money) -> Seg {
    mono(money(money_value, crate::format::Sign::Negative))
}

/// The reconciliation block (Paper `6N3-0`, `6NI-0`, `6NP-0`).
/// `exporter` names whose files have no balances ("Monzo exports").
pub fn reconciliation(preview: &ImportPreview, exporter: &str) -> Block {
    let account = &preview.account().name;
    let reconciliation = preview.reconciliation();
    match reconciliation.status() {
        ReconciliationStatus::NoBalances => Block {
            tone: Tone::Neutral,
            title: "Not reconciled".to_owned(),
            body: vec![plain(format!(
                "{exporter} carry no balances, so the totals can’t be checked against the bank. \
                 Enter today’s balance for {account} after importing to anchor it."
            ))],
        },
        ReconciliationStatus::StartingPoint => {
            let first = preview.balances().first();
            let mut body = vec![plain("The statement’s balance")];
            if let Some(balance) = first {
                body.extend([
                    plain(" on "),
                    day(balance.date),
                    plain(" is "),
                    amount(balance.balance),
                    plain(","),
                ]);
            }
            body.push(plain(format!(
                " the first {account} has, so there’s nothing to check it against yet. Later \
                 imports are checked from it."
            )));
            Block {
                tone: Tone::Neutral,
                title: "Starting point".to_owned(),
                body,
            }
        }
        ReconciliationStatus::Reconciled => {
            let latest = reconciliation
                .checks
                .iter()
                .filter(|check| matches!(check.balance, CheckedBalance::FromFile(_)))
                .max_by_key(|check| check.date)
                .or_else(|| reconciliation.checks.iter().max_by_key(|check| check.date));
            let body = match latest {
                Some(check) => vec![
                    plain("The statement’s closing balance on "),
                    day(check.date),
                    plain(" is "),
                    amount(check.stated),
                    plain(", and the imported rows add up to it exactly."),
                ],
                None => match &reconciliation.running {
                    Some(running) => vec![
                        plain("Every running balance in the file follows from the rows: "),
                        mono(count(running.checked)),
                        plain(format!(" {}.", plural(running.checked, "check"))),
                    ],
                    None => vec![plain("Every balance check passed.")],
                },
            };
            Block {
                tone: Tone::Reconciled,
                title: "Reconciled".to_owned(),
                body,
            }
        }
        ReconciliationStatus::Mismatch => Block {
            tone: Tone::Mismatch,
            title: "Balances don’t match".to_owned(),
            body: mismatch_body(preview),
        },
    }
}

/// The mismatch block's sentence: the first failing check, or the first
/// running balance that doesn't follow.
fn mismatch_body(preview: &ImportPreview) -> Vec<Seg> {
    let failed: Option<&BalanceCheck> = preview
        .reconciliation()
        .checks
        .iter()
        .filter(|check| check.after.is_failed())
        .min_by_key(|check| check.date);
    let ending = " Importing keeps the mismatch flagged on the account until it’s resolved.";
    if let Some(check) = failed
        && let CheckResult::Failed { since, .. } = check.after
        && let Some(computed) = check.computed()
    {
        let gap = Money::new(
            (computed.minor() - check.stated.minor()).abs(),
            check.stated.currency(),
        );
        let mut body = vec![
            plain("The statement says "),
            amount(check.stated),
            plain(" on "),
            day(check.date),
            plain("; the imported rows give "),
            amount(computed),
            plain(", "),
            amount(gap),
            plain(if computed.minor() < check.stated.minor() {
                " short."
            } else {
                " over."
            }),
        ];
        if since < check.date {
            body.extend([
                plain(" The gap is between "),
                day(since),
                plain(", the last balance that matched, and "),
                day(check.date),
                plain("."),
            ]);
        }
        body.push(plain(ending));
        return body;
    }
    if let Some(running) = &preview.reconciliation().running
        && let Some(first) = running.mismatches.first()
    {
        let number = preview
            .rows()
            .position(|row| row.location == first.location)
            .map_or(0, |index| index + 1);
        return vec![
            plain("Row "),
            mono(count(number)),
            plain(" says the balance is "),
            amount(first.stated),
            plain("; the rows before it give "),
            amount(first.computed),
            plain(format!(
                ".{}",
                if running.mismatches.len() > 1 {
                    format!(
                        " {} more running balances don’t follow either.",
                        running.mismatches.len() - 1
                    )
                } else {
                    String::new()
                }
            )),
            plain(ending),
        ];
    }
    vec![plain(
        "A balance doesn’t agree with the transactions.".to_owned() + ending,
    )]
}

/// The locked-period block (Paper `6NX-0`), when rows fall on or before
/// the account's lock date. Unlocking is in Reviews (M3.12).
pub fn locked(preview: &ImportPreview) -> Option<Block> {
    let mut through = None;
    let mut rows = 0;
    for row in preview.rows() {
        if let RowOutcome::Flagged(Flag::Locked { through: date }) = row.outcome {
            rows += 1;
            through = Some(*date);
        }
    }
    let through = through?;
    let others = preview.counts().new;
    Some(Block {
        tone: Tone::Locked,
        title: format!("{} {} in a locked period", count(rows), {
            if rows == 1 { "row falls" } else { "rows fall" }
        }),
        body: vec![
            plain(format!("{} is reconciled through ", preview.account().name)),
            day(through),
            plain(format!(
                ". {} out unless you unlock it in Reviews; the other ",
                if rows == 1 {
                    "This row stays"
                } else {
                    "These rows stay"
                }
            )),
            mono(count(others)),
            plain(" import as usual."),
        ],
    })
}

/// The Review step's primary button.
pub fn primary_label(preview: &ImportPreview) -> String {
    if preview.is_nothing_new() {
        return "Nothing new to import".to_owned();
    }
    if preview.needs_confirmation().balance_mismatch {
        return "Import and flag the mismatch".to_owned();
    }
    match preview.counts().new {
        0 => "Import the balance".to_owned(),
        n => format!("Import {} {}", count(n), plural(n, "transaction")),
    }
}

/// The notification after an import: `Imported 296 transactions from
/// monzo-november.csv`.
pub fn imported(transactions: usize, file: &str) -> Vec<Seg> {
    vec![
        plain("Imported "),
        mono(count(transactions)),
        plain(format!(" {} from ", plural(transactions, "transaction"))),
        mono(file),
    ]
}

/// The notification after undoing it.
pub fn undone(file: &str) -> Vec<Seg> {
    vec![plain("Undid the import of "), mono(file)]
}

#[cfg(test)]
mod tests {
    use coffer_core::money::{Currency, ParseAmountError};
    use coffer_core::statement::{Field, SubAccountKind, SubAccountRef};

    use super::*;

    fn date(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    #[test]
    fn patterns_read_as_people_write_them() {
        assert_eq!(pattern_label("%d/%m/%Y"), "DD/MM/YYYY");
        assert_eq!(pattern_label("%-d/%-m/%y"), "D/M/YY");
        assert_eq!(pattern_label("%Y-%m-%dT%H:%M:%S"), "YYYY-MM-DDTHH:mm:ss");
        assert_eq!(pattern_label("%d %b %Y"), "DD MMM YYYY");
        assert_eq!(pattern_label("%I:%M %p"), "hh:mm AM");
    }

    #[test]
    fn the_csv_caption() {
        let caption = csv_caption(TextEncoding::Utf8, Delimiter::Comma, 312);
        assert_eq!(joined(&caption), "CSV · UTF-8 · comma-separated · 312 rows");
        assert!(caption[1].mono);
    }

    #[test]
    fn pot_rows_say_which_way() {
        let mut line = StatementLine::new(date(9), Money::new(-25_000, Currency::GBP), "");
        line.sub_account = Some(SubAccountRef {
            kind: SubAccountKind::Pot,
            key: "Rainy day".to_owned(),
            name: Some("Rainy day".to_owned()),
        });
        assert_eq!(row_text(&line), ("To Rainy day".to_owned(), true));
        let mut line = StatementLine::new(date(9), Money::new(-645, Currency::GBP), "PRET 1");
        assert_eq!(row_text(&line), ("PRET 1".to_owned(), false));
        line.merchant = Some("Pret A Manger".to_owned());
        assert_eq!(row_text(&line).0, "Pret A Manger");
    }

    #[test]
    fn the_flag_note_quotes_the_files_text() {
        let problem = RowProblem::Amount {
            field: Field::Amount,
            error: ParseAmountError::Malformed("x"),
        };
        assert_eq!(
            joined(&flag_note(8, &problem, Some("8,99"), DecimalMark::Point, 0)),
            "Row 8: “8,99” isn’t an amount with a decimal point. It will be imported as \
             flagged, not skipped."
        );
        let problem = RowProblem::ColumnCount {
            expected: 3,
            found: 2,
        };
        assert_eq!(
            joined(&flag_note(2, &problem, None, DecimalMark::Point, 2)),
            "Row 2: The row has 2 columns, but the file has 3. It will be imported as flagged, \
             not skipped. 2 more rows are flagged too."
        );
    }

    #[test]
    fn pots_are_listed() {
        assert_eq!(pots_note(&[]), None);
        assert_eq!(
            pots_note(&["Holiday".to_owned(), "Rainy day".to_owned()]).unwrap(),
            "Pot transfers are paired with your Holiday and Rainy day pots automatically."
        );
        assert_eq!(
            pots_note(&["Holiday".to_owned()]).unwrap(),
            "Pot transfers are paired with your Holiday pot automatically."
        );
    }

    #[test]
    fn the_review_caption() {
        assert_eq!(
            joined(&review_caption(
                "Monzo (current account)",
                "Current account",
                Some((date(1), date(9)))
            )),
            "Monzo (current account) → Current account · 1 Nov – 9 Nov 2026"
        );
    }

    #[test]
    fn small_words() {
        assert_eq!(
            already_caption(16).unwrap(),
            "16 rows match transactions you already have, so they are skipped."
        );
        assert_eq!(already_caption(0), None);
        assert_eq!(bank_name("Monzo (current account)"), "Monzo");
        assert_eq!(bank_name("Monzo"), "Monzo");
    }
}
