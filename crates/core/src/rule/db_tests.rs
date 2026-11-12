//! Tests for storing rules and for the engine: matching, priority order,
//! and applying rules to an import batch.

use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount};
use crate::category::CategoryKey;
use crate::db::Database;
use crate::id::{AccountId, BatchId, RuleId, TransactionId};
use crate::money::Currency;
use crate::tag::TagName;

// Definitions ------------------------------------------------------------------

fn pattern(kind: MatchKind, text: &str) -> Pattern {
    Pattern::new(kind, text).unwrap()
}

fn set_category(key: &str) -> RuleAction {
    RuleAction::SetCategory(CategoryKey::new(key).unwrap())
}

fn set_merchant(name: &str) -> RuleAction {
    RuleAction::SetMerchant(ActionText::new(name).unwrap())
}

/// A rule on `field` setting `category`, with no amount or account
/// condition.
fn on(field: MatchField, kind: MatchKind, text: &str, category: &str) -> RuleDefinition {
    RuleDefinition::new(
        field,
        pattern(kind, text),
        AmountBounds::ANY,
        None,
        vec![set_category(category)],
    )
    .unwrap()
}

fn merchant(kind: MatchKind, text: &str, category: &str) -> RuleDefinition {
    on(MatchField::Merchant, kind, text, category)
}

// Matching without a database -----------------------------------------------------

/// A transaction on account 1 with only a description.
fn input(description: &str) -> MatchInput<'_> {
    MatchInput {
        account_id: AccountId::new(1),
        amount_minor: -1_000,
        merchant: None,
        description,
        source_category: None,
        source_type: None,
    }
}

fn with_merchant<'a>(name: &'a str) -> MatchInput<'a> {
    MatchInput {
        merchant: Some(name),
        ..input("CARD PAYMENT")
    }
}

/// Rules with IDs 1, 2, … and priorities 0, 1, … in the order given.
fn rule_set(rules: Vec<RuleDefinition>) -> RuleSet {
    RuleSet::new(
        rules
            .into_iter()
            .enumerate()
            .map(|(i, rule)| StoredRule::new(RuleId::new(i as i64 + 1), i as i64, rule)),
    )
}

/// The category the rules set and the rule that set it.
fn category(rules: &RuleSet, input: &MatchInput<'_>) -> Option<(String, i64)> {
    rules
        .evaluate(input)
        .category
        .map(|set| (set.value.as_str().to_owned(), set.rule.get()))
}

fn hits(rule: RuleDefinition, input: &MatchInput<'_>) -> bool {
    category(&rule_set(vec![rule]), input).is_some()
}

#[test]
fn is_matches_the_whole_field() {
    let rule = || merchant(MatchKind::Is, "tesco", "food.groceries");
    assert!(hits(rule(), &with_merchant("TESCO")));
    assert!(hits(rule(), &with_merchant("Tesco")));
    assert!(!hits(rule(), &with_merchant("TESCO STORES")));
    assert!(!hits(rule(), &with_merchant("MY TESCO")));
}

#[test]
fn starts_with_matches_a_prefix() {
    let rule = || merchant(MatchKind::StartsWith, "Tesco", "food.groceries");
    assert!(hits(rule(), &with_merchant("TESCO STORES 1234")));
    assert!(hits(rule(), &with_merchant("tesco")));
    assert!(!hits(rule(), &with_merchant("MY TESCO")));
    assert!(!hits(rule(), &with_merchant("TESC")));
}

#[test]
fn contains_matches_anywhere() {
    let rule = || merchant(MatchKind::Contains, "uber", "transport.taxi");
    assert!(hits(rule(), &with_merchant("UBER")));
    assert!(hits(rule(), &with_merchant("PAYPAL *UBER TRIP")));
    assert!(hits(rule(), &with_merchant("Uber Eats")));
    assert!(!hits(rule(), &with_merchant("U BER")));
}

#[test]
fn regex_uses_its_case_insensitive_regex() {
    let rule = || merchant(MatchKind::Regex, r"^tfl\b.*travel", "transport.public");
    assert!(hits(rule(), &with_merchant("TFL TRAVEL CH")));
    assert!(hits(rule(), &with_merchant("Tfl - Travel")));
    assert!(!hits(rule(), &with_merchant("OYSTER TFL TRAVEL")));
    assert!(!hits(rule(), &with_merchant("TFLX TRAVEL")));
}

#[test]
fn matching_ignores_case_beyond_ascii() {
    let is = || merchant(MatchKind::Is, "Café Straße", "food.eating_out");
    assert!(hits(is(), &with_merchant("CAFÉ STRASSE")));
    assert!(hits(is(), &with_merchant("café strasse")));
    assert!(!hits(is(), &with_merchant("CAFE STRASSE")));
    let contains = || merchant(MatchKind::Contains, "ÉPICERIE", "food.groceries");
    assert!(hits(contains(), &with_merchant("Mon épicerie fine")));
    let starts = || merchant(MatchKind::StartsWith, "МАГАЗИН", "food.groceries");
    assert!(hits(starts(), &with_merchant("магазин у дома")));
    let regex = || merchant(MatchKind::Regex, "^café", "food.eating_out");
    assert!(hits(regex(), &with_merchant("CAFÉ NERO")));
    assert!(!hits(regex(), &with_merchant("CAFE NERO")));
}

#[test]
fn description_is_matched_with_whitespace_collapsed() {
    let contains = || {
        on(
            MatchField::Description,
            MatchKind::Contains,
            "tesco stores",
            "food.groceries",
        )
    };
    assert!(hits(contains(), &input("CARD  TESCO\tSTORES  1234")));
    assert!(!hits(contains(), &input("TESCOSTORES")));
    let is = || {
        on(
            MatchField::Description,
            MatchKind::Is,
            "Direct Debit",
            "bills",
        )
    };
    assert!(hits(is(), &input("  DIRECT \n DEBIT ")));
    let regex = || {
        on(
            MatchField::Description,
            MatchKind::Regex,
            "^card payment to [a-z]+$",
            "shopping",
        )
    };
    assert!(hits(regex(), &input(" Card  payment to  AMAZON ")));
}

#[test]
fn each_field_matches_only_itself() {
    let row = MatchInput {
        merchant: Some("ACME"),
        source_category: Some("Groceries"),
        source_type: Some("Card payment"),
        ..input("WEEKLY SHOP")
    };
    let rule = |field, text| on(field, MatchKind::Is, text, "food.groceries");
    assert!(hits(rule(MatchField::Merchant, "acme"), &row));
    assert!(hits(rule(MatchField::Description, "weekly shop"), &row));
    assert!(hits(rule(MatchField::SourceCategory, "GROCERIES"), &row));
    assert!(hits(rule(MatchField::SourceType, "card payment"), &row));
    assert!(!hits(rule(MatchField::Merchant, "weekly shop"), &row));
    assert!(!hits(rule(MatchField::Description, "acme"), &row));
    assert!(!hits(
        rule(MatchField::SourceCategory, "card payment"),
        &row
    ));
    assert!(!hits(rule(MatchField::SourceType, "groceries"), &row));
}

#[test]
fn a_missing_field_never_matches() {
    let row = input("ANYTHING");
    for field in [
        MatchField::Merchant,
        MatchField::SourceCategory,
        MatchField::SourceType,
    ] {
        for kind in MatchKind::ALL {
            let text = if *kind == MatchKind::Regex { ".*" } else { "a" };
            assert!(
                !hits(on(field, *kind, text, "other"), &row),
                "{field} {kind}"
            );
        }
    }
    // The description is always there, so `.*` matches it.
    assert!(hits(
        on(MatchField::Description, MatchKind::Regex, ".*", "other"),
        &row
    ));
}

#[test]
fn an_account_rule_matches_only_its_account() {
    let rule = || {
        RuleDefinition::new(
            MatchField::Merchant,
            pattern(MatchKind::Contains, "acme"),
            AmountBounds::ANY,
            Some(7),
            vec![set_category("shopping")],
        )
        .unwrap()
    };
    let on_account = |id| MatchInput {
        account_id: AccountId::new(id),
        ..with_merchant("ACME")
    };
    assert!(hits(rule(), &on_account(7)));
    assert!(!hits(rule(), &on_account(8)));
}

#[test]
fn amount_bounds_are_inclusive_and_signed() {
    let rule = |min, max| {
        RuleDefinition::new(
            MatchField::Merchant,
            pattern(MatchKind::Contains, "acme"),
            AmountBounds::new(min, max).unwrap(),
            None,
            vec![set_category("shopping")],
        )
        .unwrap()
    };
    let amount = |minor| MatchInput {
        amount_minor: minor,
        ..with_merchant("ACME")
    };
    let between = || rule(Some(-5_000), Some(-100));
    for (minor, expected) in [
        (-5_001, false),
        (-5_000, true),
        (-2_500, true),
        (-100, true),
        (-99, false),
        (0, false),
        (100, false),
    ] {
        assert_eq!(hits(between(), &amount(minor)), expected, "{minor}");
    }
    // Money out only, any size.
    assert!(hits(rule(None, Some(-1)), &amount(-1_000_000)));
    assert!(!hits(rule(None, Some(-1)), &amount(0)));
    // Money in from 10.00.
    assert!(hits(rule(Some(1_000), None), &amount(1_000)));
    assert!(!hits(rule(Some(1_000), None), &amount(999)));
    assert!(!hits(rule(Some(1_000), None), &amount(-1_000)));
}

#[test]
fn the_first_matching_category_rule_wins() {
    let rules = rule_set(vec![
        merchant(MatchKind::Is, "nothing like it", "bills"),
        merchant(MatchKind::Contains, "uber eats", "food.takeaway"),
        merchant(MatchKind::Contains, "uber", "transport.taxi"),
    ]);
    assert_eq!(
        category(&rules, &with_merchant("UBER EATS")),
        Some(("food.takeaway".to_owned(), 2))
    );
    assert_eq!(
        category(&rules, &with_merchant("UBER TRIP")),
        Some(("transport.taxi".to_owned(), 3))
    );
    assert_eq!(category(&rules, &with_merchant("ACME")), None);
}

#[test]
fn rules_are_ordered_by_priority_then_id() {
    let narrow = || merchant(MatchKind::Contains, "uber eats", "food.takeaway");
    let broad = || merchant(MatchKind::Contains, "uber", "transport.taxi");
    let row = with_merchant("UBER EATS");
    // Given out of order: priority decides.
    let rules = RuleSet::new([
        StoredRule::new(RuleId::new(1), 5, broad()),
        StoredRule::new(RuleId::new(2), -3, narrow()),
    ]);
    assert_eq!(
        category(&rules, &row),
        Some(("food.takeaway".to_owned(), 2))
    );
    // Equal priorities: the lower ID first.
    let rules = RuleSet::new([
        StoredRule::new(RuleId::new(9), 0, narrow()),
        StoredRule::new(RuleId::new(4), 0, broad()),
    ]);
    assert_eq!(
        category(&rules, &row),
        Some(("transport.taxi".to_owned(), 4))
    );
}

#[test]
fn a_rule_without_a_category_doesnt_block_one_below() {
    let rename = RuleDefinition::new(
        MatchField::Merchant,
        pattern(MatchKind::Contains, "amzn"),
        AmountBounds::ANY,
        None,
        vec![set_merchant("Amazon")],
    )
    .unwrap();
    let rules = rule_set(vec![
        rename,
        merchant(MatchKind::Contains, "amzn", "shopping"),
    ]);
    assert_eq!(
        category(&rules, &with_merchant("AMZN MKTP")),
        Some(("shopping".to_owned(), 2))
    );
}

#[test]
fn a_rule_setting_several_fields_still_sets_the_category() {
    let both = RuleDefinition::new(
        MatchField::Merchant,
        pattern(MatchKind::Contains, "amzn"),
        AmountBounds::ANY,
        None,
        vec![set_merchant("Amazon"), set_category("shopping")],
    )
    .unwrap();
    let rules = rule_set(vec![both, merchant(MatchKind::Contains, "amzn", "other")]);
    assert_eq!(
        category(&rules, &with_merchant("AMZN")),
        Some(("shopping".to_owned(), 1))
    );
}

#[test]
fn an_empty_rule_set_sets_nothing() {
    let rules = RuleSet::default();
    assert!(rules.is_empty());
    assert_eq!(rules.evaluate(&with_merchant("ACME")), Outcome::default());
}

// The store ----------------------------------------------------------------------

struct Fixture {
    db: Database,
    _dir: TempDir,
    account: AccountId,
    next_fingerprint: u32,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        Fixture {
            db,
            _dir: dir,
            account,
            next_fingerprint: 0,
        }
    }

    fn create(&mut self, rule: &RuleDefinition) -> RuleId {
        create(&mut self.db, rule).unwrap()
    }

    fn order(&self) -> Vec<RuleId> {
        list(&self.db).unwrap().iter().map(StoredRule::id).collect()
    }

    fn count(&self, sql: &str) -> i64 {
        self.db.conn().query_row(sql, [], |row| row.get(0)).unwrap()
    }

    fn batch(&mut self) -> BatchId {
        self.db
            .conn()
            .execute(
                "INSERT INTO import_batch (account_id, source_file, format) \
                 VALUES (?1, 'statement.csv', 'csv')",
                [self.account],
            )
            .unwrap();
        BatchId::new(self.db.conn().last_insert_rowid())
    }

    fn category_id(&self, key: &str) -> i64 {
        self.db
            .conn()
            .query_row("SELECT id FROM category WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .unwrap()
    }

    /// An imported row in `batch` with `merchant`, optionally already
    /// categorised as `(key, source)`.
    fn row(
        &mut self,
        batch: BatchId,
        merchant: &str,
        category: Option<(&str, &str)>,
    ) -> TransactionId {
        self.next_fingerprint += 1;
        let (category_id, source) = match category {
            Some((key, source)) => (Some(self.category_id(key)), Some(source)),
            None => (None, None),
        };
        self.db
            .conn()
            .execute(
                "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
                 merchant, description_raw, description_normalized, fingerprint, origin, \
                 category_id, category_source) \
                 VALUES (?1, ?2, '2026-11-01', -1000, 'GBP', ?3, 'CARD PAYMENT', 'CARD PAYMENT', \
                 ?4, 'imported', ?5, ?6)",
                params![
                    self.account,
                    batch,
                    merchant,
                    format!("fp-{}", self.next_fingerprint),
                    category_id,
                    source,
                ],
            )
            .unwrap();
        TransactionId::new(self.db.conn().last_insert_rowid())
    }

    /// A row's category key, category source and rule.
    fn categorised(&self, id: TransactionId) -> (Option<String>, Option<String>, Option<RuleId>) {
        self.db
            .conn()
            .query_row(
                "SELECT c.key, t.category_source, t.rule_id FROM \"transaction\" t \
                 LEFT JOIN category c ON c.id = t.category_id WHERE t.id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
    }

    fn apply(&self, batch: BatchId) -> usize {
        apply_to_batch(self.db.conn(), batch).unwrap()
    }
}

fn key(text: &str) -> Option<String> {
    Some(text.to_owned())
}

#[test]
fn a_rule_round_trips() {
    let mut f = Fixture::new();
    let tags = TagList::new(vec![TagName::new("work").unwrap()]).unwrap();
    let rule = RuleDefinition::new(
        MatchField::Description,
        pattern(MatchKind::Regex, r"^tfl\s+travel"),
        AmountBounds::new(Some(-10_000), Some(-1)).unwrap(),
        Some(f.account.get()),
        vec![
            set_category("transport.public"),
            set_merchant("TfL"),
            RuleAction::AddTag(tags),
            RuleAction::MyShare(Share::new(5_000).unwrap()),
        ],
    )
    .unwrap();
    let id = f.create(&rule);
    let stored = get(&f.db, id).unwrap();
    assert_eq!(stored.id(), id);
    assert_eq!(stored.definition(), &rule);
    assert_eq!(list(&f.db).unwrap(), vec![stored]);
    assert_eq!(
        f.count("SELECT count(*) FROM rule_action"),
        rule.actions().len() as i64
    );
    assert_eq!(
        RuleSet::load(f.db.conn()).unwrap().len(),
        1,
        "the engine loads it too"
    );
}

#[test]
fn a_missing_rule_is_not_found() {
    let f = Fixture::new();
    assert!(matches!(
        get(&f.db, RuleId::new(42)),
        Err(StoreError::NotFound(id)) if id == RuleId::new(42)
    ));
}

#[test]
fn a_new_rule_goes_first() {
    let mut f = Fixture::new();
    let a = f.create(&merchant(MatchKind::Is, "a", "bills"));
    let b = f.create(&merchant(MatchKind::Is, "b", "bills"));
    let c = f.create(&merchant(MatchKind::Is, "c", "bills"));
    assert_eq!(f.order(), [c, b, a]);
    let priorities: Vec<i64> = list(&f.db)
        .unwrap()
        .iter()
        .map(StoredRule::priority)
        .collect();
    assert_eq!(priorities, [0, 1, 2]);
}

#[test]
fn a_new_rule_can_go_at_a_position() {
    let mut f = Fixture::new();
    let a = f.create(&merchant(MatchKind::Is, "a", "bills"));
    let b = f.create(&merchant(MatchKind::Is, "b", "bills"));
    let last = create_at(&mut f.db, &merchant(MatchKind::Is, "z", "bills"), 2).unwrap();
    assert_eq!(f.order(), [b, a, last]);
    let middle = create_at(&mut f.db, &merchant(MatchKind::Is, "m", "bills"), 1).unwrap();
    assert_eq!(f.order(), [b, middle, a, last]);
    assert!(matches!(
        create_at(&mut f.db, &merchant(MatchKind::Is, "x", "bills"), 5),
        Err(StoreError::NoPosition {
            position: 5,
            last: 4
        })
    ));
    assert_eq!(f.count("SELECT count(*) FROM rule"), 4);
}

#[test]
fn a_rule_naming_a_missing_category_isnt_stored() {
    let mut f = Fixture::new();
    let err = create(&mut f.db, &merchant(MatchKind::Is, "a", "food.nope")).unwrap_err();
    assert!(
        matches!(&err, StoreError::UnknownCategory(key) if key.as_str() == "food.nope"),
        "{err:?}"
    );
    let split = RuleDefinition::new(
        MatchField::Merchant,
        pattern(MatchKind::Is, "a"),
        AmountBounds::ANY,
        None,
        vec![RuleAction::SplitTemplate(
            SplitTemplate::new(vec![
                SplitLine::new(
                    CategoryKey::new("food").unwrap(),
                    Share::new(5_000).unwrap(),
                ),
                SplitLine::new(
                    CategoryKey::new("nope").unwrap(),
                    Share::new(5_000).unwrap(),
                ),
            ])
            .unwrap(),
        )],
    )
    .unwrap();
    assert!(matches!(
        create(&mut f.db, &split),
        Err(StoreError::UnknownCategory(_))
    ));
    assert_eq!(f.count("SELECT count(*) FROM rule"), 0);
    assert_eq!(f.count("SELECT count(*) FROM rule_action"), 0);
}

#[test]
fn moving_a_rule_renumbers_the_list() {
    let mut f = Fixture::new();
    let c = f.create(&merchant(MatchKind::Is, "c", "bills"));
    let b = f.create(&merchant(MatchKind::Is, "b", "bills"));
    let a = f.create(&merchant(MatchKind::Is, "a", "bills"));
    assert_eq!(f.order(), [a, b, c]);
    move_to(&mut f.db, a, 2).unwrap();
    assert_eq!(f.order(), [b, c, a]);
    move_to(&mut f.db, a, 1).unwrap();
    assert_eq!(f.order(), [b, a, c]);
    move_to(&mut f.db, c, 0).unwrap();
    assert_eq!(f.order(), [c, b, a]);
    move_to(&mut f.db, b, 1).unwrap();
    assert_eq!(f.order(), [c, b, a]);
    assert!(matches!(
        move_to(&mut f.db, a, 3),
        Err(StoreError::NoPosition {
            position: 3,
            last: 2
        })
    ));
    assert!(matches!(
        move_to(&mut f.db, RuleId::new(99), 0),
        Err(StoreError::NotFound(_))
    ));
    assert_eq!(f.order(), [c, b, a]);
}

#[test]
fn ties_in_priority_keep_id_order() {
    let mut f = Fixture::new();
    let a = f.create(&merchant(MatchKind::Is, "a", "bills"));
    let b = f.create(&merchant(MatchKind::Is, "b", "bills"));
    f.db.conn()
        .execute_batch("UPDATE rule SET priority = 7")
        .unwrap();
    assert_eq!(f.order(), [a, b]);
    move_to(&mut f.db, a, 1).unwrap();
    assert_eq!(f.order(), [b, a]);
}

#[test]
fn deleting_a_rule_keeps_what_it_set() {
    let mut f = Fixture::new();
    let gone = f.create(&merchant(MatchKind::Contains, "acme", "shopping"));
    let kept = f.create(&merchant(MatchKind::Contains, "tesco", "food.groceries"));
    let batch = f.batch();
    let acme = f.row(batch, "ACME", None);
    let tesco = f.row(batch, "TESCO", None);
    assert_eq!(f.apply(batch), 2);
    assert_eq!(
        f.categorised(acme),
        (key("shopping"), key("rule"), Some(gone))
    );

    delete(&mut f.db, gone).unwrap();
    assert_eq!(f.categorised(acme), (key("shopping"), key("rule"), None));
    assert_eq!(
        f.categorised(tesco),
        (key("food.groceries"), key("rule"), Some(kept))
    );
    assert_eq!(f.order(), [kept]);
    assert_eq!(
        f.count(&format!(
            "SELECT count(*) FROM rule_action WHERE rule_id = {gone}"
        )),
        0
    );
    assert!(matches!(
        delete(&mut f.db, gone),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn a_stored_rule_that_no_longer_validates_is_an_error_naming_its_id() {
    let mut f = Fixture::new();
    let id = f.create(&merchant(MatchKind::Is, "a", "bills"));
    f.db.conn()
        .execute_batch("UPDATE rule SET match_kind = 'regex', pattern = 'SECRET('")
        .unwrap();
    let err = list(&f.db).unwrap_err();
    assert!(
        matches!(
            &err,
            StoreError::Invalid { id: bad, problem: InvalidRule::Pattern(PatternError::InvalidRegex { .. }) }
                if *bad == id
        ),
        "{err:?}"
    );
    let message = err.to_string();
    assert!(message.starts_with(&format!("rule {id} ")), "{message}");
    assert!(!message.contains("SECRET"), "{message}");
    assert!(RuleSet::load(f.db.conn()).is_err());

    f.db.conn()
        .execute_batch(
            "UPDATE rule SET match_kind = 'is', pattern = 'a'; \
             UPDATE rule_action SET value_json = '\"SECRET KEY\"'",
        )
        .unwrap();
    let err = get(&f.db, id).unwrap_err();
    assert!(
        matches!(
            &err,
            StoreError::Invalid {
                problem: InvalidRule::Action(ActionKind::SetCategory),
                ..
            }
        ),
        "{err:?}"
    );
    assert!(!err.to_string().contains("SECRET"), "{err}");

    f.db.conn()
        .execute_batch("DELETE FROM rule_action")
        .unwrap();
    assert!(matches!(
        get(&f.db, id),
        Err(StoreError::Invalid {
            problem: InvalidRule::Definition(RuleError::NoActions),
            ..
        })
    ));
}

// Applying to a batch ------------------------------------------------------------

#[test]
fn applying_sets_uncategorised_and_preset_rows_only() {
    let mut f = Fixture::new();
    let rule = f.create(&merchant(MatchKind::Contains, "acme", "shopping"));
    let batch = f.batch();
    let open = f.row(batch, "ACME", None);
    let preset = f.row(batch, "ACME", Some(("food", "preset_map")));
    let user = f.row(batch, "ACME", Some(("bills", "user")));
    let approved = f.row(batch, "ACME", Some(("health", "ai_approved")));
    let unmatched = f.row(batch, "TESCO", None);
    let unmatched_preset = f.row(batch, "TESCO", Some(("food", "preset_map")));
    let other_batch = f.batch();
    let elsewhere = f.row(other_batch, "ACME", None);

    assert_eq!(f.apply(batch), 2);
    let by_rule = (key("shopping"), key("rule"), Some(rule));
    assert_eq!(f.categorised(open), by_rule);
    assert_eq!(f.categorised(preset), by_rule);
    assert_eq!(f.categorised(user), (key("bills"), key("user"), None));
    assert_eq!(
        f.categorised(approved),
        (key("health"), key("ai_approved"), None)
    );
    assert_eq!(f.categorised(unmatched), (None, None, None));
    assert_eq!(
        f.categorised(unmatched_preset),
        (key("food"), key("preset_map"), None)
    );
    assert_eq!(f.categorised(elsewhere), (None, None, None));

    // A second run finds the rows already set by the rule.
    assert_eq!(f.apply(batch), 0);
}

#[test]
fn applying_uses_the_highest_priority_rule() {
    let mut f = Fixture::new();
    let broad = f.create(&merchant(MatchKind::Contains, "uber", "transport.taxi"));
    // Created second, so it goes first.
    let narrow = f.create(&merchant(MatchKind::Contains, "uber eats", "food.takeaway"));
    let batch = f.batch();
    let eats = f.row(batch, "UBER EATS", None);
    let trip = f.row(batch, "UBER TRIP", None);
    assert_eq!(f.apply(batch), 2);
    assert_eq!(
        f.categorised(eats),
        (key("food.takeaway"), key("rule"), Some(narrow))
    );
    assert_eq!(
        f.categorised(trip),
        (key("transport.taxi"), key("rule"), Some(broad))
    );
}

#[test]
fn applying_with_no_rules_changes_nothing() {
    let mut f = Fixture::new();
    let batch = f.batch();
    let row = f.row(batch, "ACME", None);
    assert_eq!(f.apply(batch), 0);
    assert_eq!(f.categorised(row), (None, None, None));
}

#[test]
fn applying_runs_inside_the_callers_transaction() {
    let mut f = Fixture::new();
    f.create(&merchant(MatchKind::Contains, "acme", "shopping"));
    let batch = f.batch();
    let row = f.row(batch, "ACME", None);
    {
        let tx = f.db.conn_mut().transaction().unwrap();
        assert_eq!(apply_to_batch(&tx, batch).unwrap(), 1);
        // Dropped without commit: rolled back.
    }
    assert_eq!(f.categorised(row), (None, None, None));
}

#[test]
fn applying_a_rule_whose_category_is_gone_is_an_error() {
    let mut f = Fixture::new();
    let rule = f.create(&merchant(MatchKind::Contains, "acme", "shopping"));
    f.db.conn()
        .execute_batch("UPDATE rule_action SET value_json = '\"shopping.gone\"'")
        .unwrap();
    let batch = f.batch();
    let row = f.row(batch, "ACME", None);
    let err = apply_to_batch(f.db.conn(), batch).unwrap_err();
    assert!(
        matches!(&err, StoreError::RuleCategoryMissing { rule: r, key } if *r == rule && key.as_str() == "shopping.gone"),
        "{err:?}"
    );
    assert_eq!(f.categorised(row), (None, None, None));
    // A rule that matches nothing doesn't need its category.
    let other = f.batch();
    f.row(other, "TESCO", None);
    assert_eq!(f.apply(other), 0);
}
