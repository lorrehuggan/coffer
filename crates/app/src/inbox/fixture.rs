//! Sample suggestions for the Inbox's snapshots and tests: the D6.1/D6.6
//! fixture (Paper `564-0`, `ABK-0`, `DL8-0`, `EWZ-0`), written into a
//! database through core's own API, so what the screen shows comes from the
//! real store and previews. Every merchant, amount and reason is invented.

use std::collections::BTreeSet;

use chrono::NaiveDate;
use coffer_core::account::{self, AccountKind, NewAccount};
use coffer_core::category::CategoryKey;
use coffer_core::db::Database;
use coffer_core::id::{AccountId, TransactionId};
use coffer_core::inbox::{self, NewSuggestion, SuggestionKind, own};
use coffer_core::manual::{self, ManualTransaction};
use coffer_core::mcp_client::{self, ClientName};
use coffer_core::merchant::{self, DisplayName};
use coffer_core::money::{Currency, Money};
use coffer_core::rule::{self, AmountBounds, MatchField, MatchKind, Pattern, RuleAction};
use serde_json::{Value, json};

/// The agent the sample suggestions come from.
pub const AGENT: &str = "claude-code";

fn date(month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, day).expect("a fixture date")
}

fn key(key: &str) -> CategoryKey {
    CategoryKey::new(key).expect("a fixture category key")
}

struct Seed<'a> {
    db: &'a mut Database,
    current: AccountId,
}

impl Seed<'_> {
    fn row_on(
        &mut self,
        account: AccountId,
        (month, day): (u32, u32),
        minor: i64,
        merchant: &str,
        bank_text: &str,
        category: Option<&str>,
    ) -> TransactionId {
        let mut entry = ManualTransaction::new(
            account,
            date(month, day),
            Money::new(minor, Currency::GBP),
            bank_text,
        );
        entry.merchant = Some(merchant.to_owned());
        entry.category = category.map(key);
        manual::create(self.db, &entry).expect("a fixture row")
    }

    fn row(
        &mut self,
        day: (u32, u32),
        minor: i64,
        merchant: &str,
        category: Option<&str>,
    ) -> TransactionId {
        self.row_on(self.current, day, minor, merchant, merchant, category)
    }

    fn file(&mut self, kind: SuggestionKind, payload: Value, rationale: &str) {
        inbox::file_suggestion(
            self.db,
            &ClientName::new(AGENT).expect("the fixture agent's name"),
            &NewSuggestion {
                kind,
                payload,
                rationale: Some(rationale.to_owned()),
            },
        )
        .expect("a fixture suggestion");
    }

    fn categorise(&mut self, transaction: TransactionId, category: &str, rationale: &str) {
        self.file(
            SuggestionKind::Categorization,
            json!({"items": [{"transaction_id": transaction.get(), "category": category}]}),
            rationale,
        );
    }
}

/// Writes the sample accounts, transactions, rules and suggestions into
/// `db`, which should be empty.
pub fn seed(db: &mut Database) {
    let current = account::create(
        db,
        &NewAccount::new("Current account", AccountKind::Current, Currency::GBP),
    )
    .expect("the fixture's current account");
    let savings = account::create(
        db,
        &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
    )
    .expect("the fixture's savings account");
    for client in [AGENT, "codex"] {
        mcp_client::enable(db, &ClientName::new(client).expect("a client name"))
            .expect("enabling a fixture client");
    }
    let mut s = Seed { db, current };

    // Rule UBER files every UBER row under Transport.
    let uber = [
        ((11, 4), -1840, "UBER *EATS HELP.UBER.COM NL"),
        ((10, 27), -2215, "UBER *EATS HELP.UBER.COM NL"),
        ((10, 12), -1680, "UBER *EATS UBER.COM/BILL NL"),
        ((9, 19), -1960, "UBER *EATS HELP.UBER.COM NL"),
        ((9, 2), -1425, "UBER *EATS HELP.UBER.COM NL"),
    ];
    let mut uber_rows = Vec::new();
    for (day, minor, text) in uber {
        uber_rows.push(s.row_on(current, day, minor, "UBER *EATS", text, None));
    }
    s.row_on(
        current,
        (10, 3),
        -2310,
        "UBER *EATS",
        "UBER *EATS HELP.UBER.COM NL",
        Some("food.groceries"),
    );
    let uber_rule = rule::create(
        s.db,
        &rule::RuleDefinition::new(
            MatchField::Merchant,
            Pattern::new(MatchKind::Contains, "UBER").expect("a pattern"),
            AmountBounds::default(),
            None,
            vec![RuleAction::SetCategory(key("transport"))],
        )
        .expect("the fixture rule"),
    )
    .expect("storing the fixture rule");
    let plan = rule::preview_apply(s.db, uber_rule).expect("planning the fixture rule");
    rule::commit_apply(s.db, &plan, &BTreeSet::new()).expect("applying the fixture rule");

    for (day, minor) in [((11, 2), -2745), ((10, 20), -3110), ((10, 6), -1890)] {
        s.row_on(
            current,
            day,
            minor,
            "DELIVEROO",
            "DELIVEROO.CO.UK LONDON",
            None,
        );
    }
    let lime = [
        ((11, 6), -345, "LIME*RIDE 3HX9QP LONDON"),
        ((10, 29), -510, "LIME*RIDE 7KD2LM LONDON"),
        ((10, 21), -285, "LIME*RIDE Q81ZTA LONDON"),
        ((10, 14), -1499, "LIME*RIDE PASS 30D"),
        ((10, 9), -420, "LIME*RIDE 0WP4NC LONDON"),
        ((9, 28), -675, "LIME*RIDE H6R2YB LONDON"),
    ];
    for (day, minor, text) in lime {
        let merchant = text.trim_end_matches(" LONDON");
        let row = s.row_on(current, day, minor, merchant, text, None);
        if text.contains("PASS") {
            merchant::set_user_name(
                s.db,
                row,
                &DisplayName::new("Lime monthly pass").expect("a name"),
            )
            .expect("naming the pass");
        }
    }
    let to_current = s.row_on(
        savings,
        (11, 4),
        -25_000,
        "To Current account",
        "TO CURRENT",
        None,
    );
    let from_savings = s.row((11, 4), 25_000, "From Savings", None);
    let shirt = s.row((10, 24), -5980, "Uniqlo", Some("shopping"));
    let money_back = s.row((11, 7), 2990, "Uniqlo", None);
    let to_categorise = [
        ((11, 9), -2399, "Amazon Marketplace", "shopping"),
        ((11, 8), -2745, "Deliveroo", "food.eating_out"),
        ((11, 6), -560, "Gail's Bakery", "food.eating_out"),
        ((11, 2), -1135, "CO-OP GROUP 070512", "food.groceries"),
        ((11, 1), -1699, "Waterstones", "shopping"),
        ((10, 31), -2000, "LUL TICKET MACHINE", "transport"),
        (
            (10, 30),
            -2450,
            "Everyman Cinema, Kings Cross",
            "entertainment",
        ),
        ((10, 29), -875, "itsu", "food.eating_out"),
    ];
    let to_categorise: Vec<(TransactionId, &str)> = to_categorise
        .into_iter()
        .map(|(day, minor, merchant, category)| (s.row(day, minor, merchant, None), category))
        .collect();
    // Three hand edits to WASABI: Coffer suggests a rule itself.
    let wasabi: Vec<TransactionId> = [(10, 15), (10, 22), (11, 5)]
        .into_iter()
        .map(|day| s.row(day, -895, "WASABI", None))
        .collect();
    for row in wasabi {
        own::recategorize(s.db, &[(row, key("food.eating_out"))]).expect("a hand edit");
    }

    s.file(
        SuggestionKind::Rule,
        rule_json("DELIVEROO", "food.eating_out"),
        "Deliveroo is food delivery; every one of these is a takeaway.",
    );
    s.file(
        SuggestionKind::Rule,
        rule_json("UBER *EATS", "food.eating_out"),
        "Uber Eats is food delivery, but your rule for UBER files it with taxi rides. This \
         rule would decide first.",
    );
    s.file(
        SuggestionKind::MerchantAlias,
        json!({"match_kind": "prefix", "pattern": "LIME*RIDE", "display_name": "Lime"}),
        "Lime adds a ride code to every payment, so each ride shows up as a different \
         merchant in Trends.",
    );
    s.file(
        SuggestionKind::TransferPair,
        json!({"outflow_id": to_current.get(), "inflow_id": from_savings.get()}),
        "£250.00 left Savings and reached Current account the same day, with matching \
         references.",
    );
    s.file(
        SuggestionKind::RefundLink,
        json!({"refund_id": money_back.get(), "purchase_id": shirt.get(), "link_kind": "refund"}),
        "Same shop, 14 days later, exactly half of the £59.80 purchase: one of two items \
         sent back.",
    );
    for (row, category) in to_categorise {
        s.categorise(row, category, "The merchant's name says what it sells.");
    }
    s.categorise(
        uber_rows[0],
        "food.eating_out",
        "Uber Eats is food delivery. Your rule for UBER files it with taxi rides.",
    );
}

fn rule_json(pattern: &str, category: &str) -> Value {
    json!({"match_field": "merchant", "match_kind": "contains", "pattern": pattern,
           "actions": [{"kind": "set_category", "value": category}]})
}

/// Adds D6.6's wide rule: "Description contains PAY" → Bills, over the
/// salary and more than five merchants (states board `EWZ-0`).
pub fn seed_wide_rule(db: &mut Database) {
    let current = account::create(
        db,
        &NewAccount::new("Joint account", AccountKind::Current, Currency::GBP),
    )
    .expect("the wide rule's account");
    let mut s = Seed { db, current };
    s.row((10, 28), 255_000, "ACME LTD PAYROLL", None);
    for (index, name) in [
        "PAYPAL *EBAY",
        "TFL PAYG",
        "PAYPOINT",
        "PAYZONE",
        "APPLEPAY TOPUP",
        "PAYBYPHONE",
    ]
    .into_iter()
    .enumerate()
    {
        for day in [3, 10, 17] {
            s.row(
                (10, day + index as u32),
                -1250 - 100 * index as i64,
                name,
                None,
            );
        }
    }
    s.row((10, 30), -4023, "PAYPAL *EBAY", Some("shopping"));
    s.file(
        SuggestionKind::Rule,
        json!({"match_field": "description", "match_kind": "contains", "pattern": "PAY",
               "actions": [{"kind": "set_category", "value": "bills"}]}),
        "Most of these are bills.",
    );
}
