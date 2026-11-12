//! Tests for merchant names: the clean-up, alias resolution, storing
//! aliases and recomputing names. Every merchant here is made up.

use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount};
use crate::db::Database;
use crate::id::AccountId;
use crate::money::Currency;
use crate::rule::{MAX_PATTERN_LEN, PatternError};

// Clean-up ---------------------------------------------------------------------

#[track_caller]
fn cleans(input: &str, expected: &str) {
    assert_eq!(clean(input), expected, "{input:?}");
}

#[test]
fn strips_processor_prefixes() {
    cleans("SQ *ACME COFFEE", "ACME COFFEE");
    cleans("SQ*ACME COFFEE", "ACME COFFEE");
    cleans("Sq *Acme Coffee", "Acme Coffee");
    cleans("SUMUP *ACME BAKERY", "ACME BAKERY");
    cleans("SUMUP  *ACME BAKERY", "ACME BAKERY");
    cleans("ZETTLE_*ACME STALL", "ACME STALL");
    cleans("ZETTLE_ACME STALL", "ACME STALL");
    cleans("IZ *ACME CRAFTS", "ACME CRAFTS");
    cleans("iZ *Acme Crafts", "Acme Crafts");
    cleans("PAYPAL *ACMESTORE", "ACMESTORE");
    cleans("PP*ACMESTORE", "ACMESTORE");
    cleans("CRV*ACME PARKING", "ACME PARKING");
    cleans("TST* ACME DINER", "ACME DINER");
    cleans("SP * ACME SHOP", "ACME SHOP");
    cleans("SP ACME SHOP", "ACME SHOP");
    // Only a prefix: a word that starts with the letters stays.
    cleans("SPAR ACME", "SPAR ACME");
    cleans("SQUARE MEAL", "SQUARE MEAL");
    cleans("PAYPALS ACME", "PAYPALS ACME");
    cleans("ACME SQ *", "ACME SQ");
}

#[test]
fn strips_payment_method_words() {
    cleans("CONTACTLESS ACME CAFE", "ACME CAFE");
    cleans("ACME CAFE CONTACTLESS", "ACME CAFE");
    cleans("ACME CAFE - CONTACTLESS PAYMENT", "ACME CAFE");
    cleans("CARD PAYMENT TO ACME CAFE", "ACME CAFE");
    cleans("CARD PURCHASE ACME CAFE", "ACME CAFE");
    cleans("DEBIT CARD PAYMENT ACME CAFE", "ACME CAFE");
    cleans("POS ACME CAFE", "ACME CAFE");
    cleans("VIS ACME CAFE", "ACME CAFE");
    cleans("APPLE PAY ACME CAFE", "ACME CAFE");
    cleans("Acme Cafe Google Pay", "Acme Cafe");
    cleans("ACME CAFE CHIP AND PIN", "ACME CAFE");
    // Whole words only, and not inside the name.
    cleans("POSTAGE ACME", "POSTAGE ACME");
    cleans("VISTA ACME", "VISTA ACME");
    cleans("ACME POS SYSTEMS", "ACME POS SYSTEMS");
    cleans("ACME CONTACTLESS LENSES", "ACME CONTACTLESS LENSES");
}

#[test]
fn strips_card_references_and_masked_numbers() {
    cleans("ACME CAFE CARD 1234", "ACME CAFE");
    cleans("CARD 1234 ACME CAFE", "ACME CAFE");
    cleans("ACME CAFE CARD ENDING 1234", "ACME CAFE");
    cleans("ACME CAFE CARD ENDING IN 1234", "ACME CAFE");
    cleans("ACME CAFE CARD NO. XXXX1234", "ACME CAFE");
    cleans("ACME CAFE CARD: ****1234", "ACME CAFE");
    cleans("ACME CAFE XXXX XXXX XXXX 1234", "ACME CAFE");
    cleans("ACME CAFE ****1234", "ACME CAFE");
    cleans("ACME CAFE 4929********1234 LONDON", "ACME CAFE LONDON");
    cleans("ACME CAFE xxxx1234", "ACME CAFE");
    // Not a card: letters in the word, or too few masks.
    cleans("ACME XXL", "ACME XXL");
    cleans("ACME XX", "ACME XX");
    cleans("ACME CARDS", "ACME CARDS");
}

#[test]
fn strips_trailing_dates_times_and_references() {
    cleans("ACME CAFE 12/11", "ACME CAFE");
    cleans("ACME CAFE ON 12/11", "ACME CAFE");
    cleans("ACME CAFE 12/11/26", "ACME CAFE");
    cleans("ACME CAFE 1.11.2026", "ACME CAFE");
    cleans("ACME CAFE 2026-11-12", "ACME CAFE");
    cleans("ACME CAFE 12NOV", "ACME CAFE");
    cleans("ACME CAFE 12NOV26", "ACME CAFE");
    cleans("ACME CAFE ON 12 NOV", "ACME CAFE");
    cleans("ACME CAFE 2 September 2026", "ACME CAFE");
    cleans("ACME CAFE 10:42", "ACME CAFE");
    cleans("ACME CAFE 12/11 10:42:05", "ACME CAFE");
    cleans("ACME STORES 3297", "ACME STORES");
    cleans("ACME STORES #3297", "ACME STORES");
    cleans("PAYPAL *ACMESTORE 4029357733", "ACMESTORE");
    cleans("ACME LTD REF 123456", "ACME LTD");
    cleans("ACME LTD REF: AB1234", "ACME LTD");
    cleans("ACME LTD AB123456", "ACME LTD");
    cleans("ACME.EXAMPLE*AB12CD34E", "ACME.EXAMPLE");
    // Kept: not a date or a long digit run, or not at the end.
    cleans("ACME 24/7", "ACME 24/7");
    cleans("ACME 12 MAYFAIR", "ACME 12 MAYFAIR");
    cleans("ACME 123", "ACME 123");
    cleans("ACME 3297 STORES", "ACME 3297 STORES");
    cleans("UBER *TRIP", "UBER *TRIP");
    cleans("ACME REF", "ACME REF");
    cleans("ACME W1A1AA", "ACME W1A1AA");
    // The first word always stays.
    cleans("12345", "12345");
    cleans("12/11", "12/11");
}

#[test]
fn combines_and_collapses() {
    cleans(
        "  CONTACTLESS  SQ *ACME   COFFEE  LONDON 12/11 CARD 1234 ",
        "ACME COFFEE LONDON",
    );
    cleans("ACME CAFE\t\t12NOV\n", "ACME CAFE");
    cleans("SQ *ACME COFFEE *", "ACME COFFEE");
    cleans("ACME CAFE -", "ACME CAFE");
    cleans("SQ *CAFÉ ÉTOILE 12/11", "CAFÉ ÉTOILE");
    cleans("ACME LTD.", "ACME LTD.");
}

#[test]
fn keeps_the_case() {
    cleans("Acme Coffee", "Acme Coffee");
    cleans("acme coffee", "acme coffee");
    cleans("ACME eShop", "ACME eShop");
}

#[test]
fn never_empties_non_blank_text() {
    for input in [
        "CONTACTLESS",
        "SQ *",
        "XXXX1234",
        "XXXX XXXX XXXX 1234",
        "CARD 1234",
        "*",
        " - ",
        "  CONTACTLESS   PAYMENT  ",
        "12345",
    ] {
        assert!(!clean(input).is_empty(), "{input:?}");
    }
    // When nothing would be left, the text is kept whole.
    cleans("CONTACTLESS", "CONTACTLESS");
    cleans("  SQ  *  ", "SQ *");
    cleans("XXXX XXXX XXXX 1234", "XXXX XXXX XXXX 1234");
    cleans("", "");
    cleans(" \t ", "");
}

#[test]
fn cleaning_is_idempotent() {
    for input in [
        "SQ *ACME COFFEE",
        "CONTACTLESS ACME CAFE 12/11",
        "ACME CAFE XXXX XXXX XXXX 1234",
        "PAYPAL *ACMESTORE 4029357733",
        "ACME LTD REF 123456",
        "Acme Coffee",
        "CONTACTLESS",
        "SQ *",
        "12345",
        "ACME 24/7",
        "ACME.EXAMPLE*AB12CD34E",
        "  CONTACTLESS  SQ *ACME   COFFEE  LONDON 12/11 CARD 1234 ",
        "",
    ] {
        let once = clean(input);
        assert_eq!(clean(&once), once, "{input:?}");
    }
}

#[test]
fn source_text_prefers_a_non_blank_merchant() {
    assert_eq!(source_text(Some("Acme"), "CARD ACME"), Some("Acme"));
    assert_eq!(source_text(None, "CARD ACME"), Some("CARD ACME"));
    assert_eq!(source_text(Some("  "), "CARD ACME"), Some("CARD ACME"));
    assert_eq!(source_text(Some(""), " "), None);
    assert_eq!(source_text(None, ""), None);
}

// Definitions and resolution -------------------------------------------------------

fn name(text: &str) -> DisplayName {
    DisplayName::new(text).unwrap()
}

fn definition(kind: AliasKind, pattern: &str, shows: &str) -> AliasDefinition {
    AliasDefinition::new(kind, pattern, name(shows)).unwrap()
}

/// Aliases with IDs 1, 2, … in the order given.
fn alias_set(aliases: &[(AliasKind, &str, &str)]) -> AliasSet {
    AliasSet::new(
        aliases
            .iter()
            .enumerate()
            .map(|(i, (kind, pattern, shows))| {
                MerchantAlias::new(
                    MerchantAliasId::new(i as i64 + 1),
                    definition(*kind, pattern, shows),
                )
            }),
    )
}

/// The name and the alias's ID.
fn resolve(set: &AliasSet, cleaned: &str) -> (String, Option<i64>) {
    let resolved = set.resolve(cleaned);
    (resolved.name.to_owned(), resolved.alias.map(|id| id.get()))
}

#[test]
fn keys_match_the_schema() {
    let sql = include_str!("../../migrations/V7__merchant_alias.sql");
    let keys: Vec<String> = AliasKind::ALL
        .iter()
        .map(|kind| format!("'{kind}'"))
        .collect();
    assert!(sql.contains(&format!("match_kind IN ({})", keys.join(", "))));
    for kind in AliasKind::ALL {
        assert_eq!(kind.as_str().parse::<AliasKind>(), Ok(*kind));
    }
    assert!("Exact".parse::<AliasKind>().is_err());
}

#[test]
fn definitions_are_validated_without_echoing_them() {
    assert_eq!(DisplayName::new(" \t"), Err(AliasError::BlankName));
    assert_eq!(name("  Acme  ").as_str(), "Acme");
    assert_eq!(format!("{:?}", name("SECRET")), "DisplayName { len: 6 }");
    for kind in AliasKind::ALL {
        assert_eq!(
            AliasDefinition::new(*kind, "  ", name("Acme")),
            Err(AliasError::BlankPattern)
        );
        assert_eq!(
            AliasDefinition::new(*kind, &"a".repeat(MAX_PATTERN_LEN + 1), name("Acme")),
            Err(AliasError::Pattern(PatternError::TooLong {
                max: MAX_PATTERN_LEN
            }))
        );
    }
    for pattern in ["SECRET(", "SECRET[", r"\qSECRET"] {
        let err = AliasDefinition::new(AliasKind::Regex, pattern, name("Acme")).unwrap_err();
        assert!(matches!(
            err,
            AliasError::Pattern(PatternError::InvalidRegex { .. })
        ));
        assert!(!err.to_string().contains("SECRET"), "{err}");
    }
    assert_eq!(
        AliasDefinition::new(AliasKind::Regex, r"\w{1000}", name("Acme")),
        Err(AliasError::Pattern(PatternError::RegexTooBig))
    );
    // Plain patterns are text, not regexes.
    assert!(AliasDefinition::new(AliasKind::Exact, "ACME (", name("Acme")).is_ok());
}

#[test]
fn no_alias_keeps_the_cleaned_text() {
    let set = AliasSet::default();
    assert!(set.is_empty());
    assert_eq!(resolve(&set, "ACME COFFEE"), ("ACME COFFEE".into(), None));
    let set = alias_set(&[(AliasKind::Exact, "OTHER", "Other")]);
    assert_eq!(resolve(&set, "ACME COFFEE"), ("ACME COFFEE".into(), None));
}

#[test]
fn exact_ignores_case_and_repeated_whitespace() {
    let set = alias_set(&[(AliasKind::Exact, "acme  coffee", "Acme Coffee")]);
    assert_eq!(
        resolve(&set, "ACME COFFEE"),
        ("Acme Coffee".into(), Some(1))
    );
    assert_eq!(resolve(&set, "Acme Coffee").1, Some(1));
    assert_eq!(resolve(&set, "ACME COFFEE LONDON").1, None);
    assert_eq!(resolve(&set, "MY ACME COFFEE").1, None);
    let set = alias_set(&[(AliasKind::Exact, "Café Straße", "Café")]);
    assert_eq!(resolve(&set, "CAFÉ STRASSE").1, Some(1));
}

#[test]
fn exact_wins_over_prefix_and_regex() {
    let set = alias_set(&[
        (AliasKind::Regex, "^acme", "By regex"),
        (AliasKind::Prefix, "ACME", "By prefix"),
        (AliasKind::Exact, "ACME COFFEE", "By exact"),
    ]);
    assert_eq!(resolve(&set, "ACME COFFEE"), ("By exact".into(), Some(3)));
    assert_eq!(resolve(&set, "ACME BAKERY"), ("By prefix".into(), Some(2)));
}

#[test]
fn the_longest_prefix_wins() {
    let set = alias_set(&[
        (AliasKind::Prefix, "ACME", "Acme"),
        (AliasKind::Prefix, "acme coffee", "Acme Coffee"),
        (AliasKind::Prefix, "ACME COFFEE ROASTERS", "Acme Roasters"),
    ]);
    assert_eq!(resolve(&set, "ACME BAKERY").1, Some(1));
    assert_eq!(resolve(&set, "ACME COFFEE LONDON").1, Some(2));
    assert_eq!(resolve(&set, "Acme Coffee Roasters Ltd").1, Some(3));
    assert_eq!(resolve(&set, "ACM").1, None);
}

#[test]
fn a_prefix_trailing_space_marks_a_word() {
    let set = alias_set(&[(AliasKind::Prefix, "ACME ", "Acme")]);
    assert_eq!(resolve(&set, "ACME STORE").1, Some(1));
    assert_eq!(resolve(&set, "ACMEX STORE").1, None);
    assert_eq!(resolve(&set, "ACME").1, None);
}

#[test]
fn regexes_go_in_id_order_over_the_cleaned_text() {
    let set = alias_set(&[
        (AliasKind::Regex, r"coffee$", "First"),
        (AliasKind::Regex, r"^acme", "Second"),
    ]);
    assert_eq!(resolve(&set, "ACME COFFEE"), ("First".into(), Some(1)));
    assert_eq!(resolve(&set, "ACME BAKERY"), ("Second".into(), Some(2)));
    // The set's order is the IDs', whatever order it is given in.
    let reversed = AliasSet::new([
        MerchantAlias::new(
            MerchantAliasId::new(2),
            definition(AliasKind::Regex, "^acme", "Second"),
        ),
        MerchantAlias::new(
            MerchantAliasId::new(1),
            definition(AliasKind::Regex, "coffee$", "First"),
        ),
    ]);
    assert_eq!(resolve(&reversed, "ACME COFFEE").1, Some(1));
}

#[test]
fn display_name_cleans_then_resolves() {
    let set = alias_set(&[(AliasKind::Exact, "ACME COFFEE", "Acme Coffee")]);
    assert_eq!(
        set.display_name(Some("SQ *ACME COFFEE"), "ignored"),
        Some("Acme Coffee".into())
    );
    assert_eq!(
        set.display_name(None, "CONTACTLESS ACME COFFEE 12/11"),
        Some("Acme Coffee".into())
    );
    assert_eq!(
        set.display_name(Some(" "), "CARD PAYMENT TO ACME BAKERY"),
        Some("ACME BAKERY".into())
    );
    assert_eq!(set.display_name(None, " "), None);
}

// Storing and recomputing ------------------------------------------------------

struct Fixture {
    db: Database,
    account: AccountId,
    _dir: TempDir,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        Fixture {
            db,
            account,
            _dir: dir,
        }
    }

    fn batch(&self) -> BatchId {
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

    /// An imported row with this bank text.
    fn row(&self, batch: BatchId, merchant: Option<&str>, description: &str) -> TransactionId {
        let conn = self.db.conn();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM \"transaction\"", [], |row| row.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
             merchant, description_raw, description_normalized, fingerprint, origin) \
             VALUES (?1, ?2, '2026-11-02', -450, 'GBP', ?3, ?4, upper(?4), ?5, 'imported')",
            params![
                self.account,
                batch,
                merchant,
                description,
                format!("fp-{n}")
            ],
        )
        .unwrap();
        TransactionId::new(conn.last_insert_rowid())
    }

    fn shown(&self, id: TransactionId) -> (Option<String>, Option<String>) {
        self.db
            .conn()
            .query_row(
                "SELECT merchant_display, merchant_display_source FROM \"transaction\" \
                 WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    }

    fn name(&self, id: TransactionId) -> Option<String> {
        self.shown(id).0
    }

    /// The bank's own fields of a row.
    fn source_fields(&self, id: TransactionId) -> (Option<String>, String, String) {
        self.db
            .conn()
            .query_row(
                "SELECT merchant, description_raw, description_normalized \
                 FROM \"transaction\" WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
    }

    fn set_rule_name(&self, id: TransactionId, name: &str) {
        self.db
            .conn()
            .execute(
                "UPDATE \"transaction\" SET merchant_display = ?1, \
                 merchant_display_source = 'rule' WHERE id = ?2",
                params![name, id],
            )
            .unwrap();
    }
}

#[test]
fn apply_to_batch_names_only_the_batch_and_derived_rows() {
    let mut f = Fixture::new();
    let first = f.batch();
    let a = f.row(first, Some("SQ *ACME COFFEE"), "SQ *ACME COFFEE LONDON");
    let b = f.row(first, None, "CONTACTLESS ACME BAKERY 12/11");
    let c = f.row(first, None, "");
    let by_rule = f.row(first, Some("ACME BOOKS"), "ACME BOOKS");
    f.set_rule_name(by_rule, "Books");
    let second = f.batch();
    let other = f.row(second, Some("ACME GARAGE"), "ACME GARAGE");
    add(
        &mut f.db,
        &definition(AliasKind::Exact, "ACME COFFEE", "Acme Coffee"),
    )
    .unwrap();
    // Adding the alias named every derived row; clear them to test the
    // batch on its own.
    f.db.conn()
        .execute(
            "UPDATE \"transaction\" SET merchant_display = NULL \
             WHERE merchant_display_source IS NULL",
            [],
        )
        .unwrap();

    assert_eq!(apply_to_batch(f.db.conn(), first).unwrap(), 2);
    assert_eq!(f.name(a).as_deref(), Some("Acme Coffee"));
    assert_eq!(f.name(b).as_deref(), Some("ACME BAKERY"));
    assert_eq!(f.name(c), None);
    assert_eq!(
        f.shown(by_rule),
        (Some("Books".into()), Some("rule".into()))
    );
    assert_eq!(f.name(other), None);
    // Nothing left to change.
    assert_eq!(apply_to_batch(f.db.conn(), first).unwrap(), 0);
}

#[test]
fn alias_changes_apply_at_once_to_derived_rows_only() {
    let mut f = Fixture::new();
    let batch = f.batch();
    let derived = f.row(batch, Some("SQ *ACME COFFEE"), "x");
    let other = f.row(batch, Some("ACME BAKERY"), "x");
    let by_rule = f.row(batch, Some("ACME COFFEE"), "x");
    let by_user = f.row(batch, Some("ACME COFFEE"), "x");
    assert_eq!(recompute_all(f.db.conn()).unwrap(), 4);
    f.set_rule_name(by_rule, "From a rule");
    set_user_name(&mut f.db, by_user, &name("Mine")).unwrap();
    let before = [derived, other, by_rule, by_user].map(|id| f.source_fields(id));

    let (id, changed) = add(
        &mut f.db,
        &definition(AliasKind::Prefix, "acme coffee", "Acme Coffee"),
    )
    .unwrap();
    assert_eq!(changed, 1);
    assert_eq!(f.name(derived).as_deref(), Some("Acme Coffee"));
    assert_eq!(f.name(other).as_deref(), Some("ACME BAKERY"));
    assert_eq!(
        f.shown(by_rule),
        (Some("From a rule".into()), Some("rule".into()))
    );
    assert_eq!(f.shown(by_user), (Some("Mine".into()), Some("user".into())));

    let changed = edit(
        &mut f.db,
        id,
        &definition(AliasKind::Prefix, "ACME", "Acme"),
    )
    .unwrap();
    assert_eq!(changed, 2);
    assert_eq!(f.name(derived).as_deref(), Some("Acme"));
    assert_eq!(f.name(other).as_deref(), Some("Acme"));
    assert_eq!(f.name(by_user).as_deref(), Some("Mine"));

    // Editing to the same thing changes nothing.
    assert_eq!(
        edit(
            &mut f.db,
            id,
            &definition(AliasKind::Prefix, "ACME", "Acme")
        )
        .unwrap(),
        0
    );

    assert_eq!(delete(&mut f.db, id).unwrap(), 2);
    assert_eq!(f.name(derived).as_deref(), Some("ACME COFFEE"));
    assert_eq!(f.name(other).as_deref(), Some("ACME BAKERY"));
    assert_eq!(f.name(by_rule).as_deref(), Some("From a rule"));

    // The bank's text is never changed.
    let after = [derived, other, by_rule, by_user].map(|id| f.source_fields(id));
    assert_eq!(before, after);
}

#[test]
fn a_user_name_holds_until_cleared() {
    let mut f = Fixture::new();
    let batch = f.batch();
    let id = f.row(batch, None, "CONTACTLESS ACME COFFEE");
    set_user_name(&mut f.db, id, &name(" Coffee ")).unwrap();
    assert_eq!(f.shown(id), (Some("Coffee".into()), Some("user".into())));
    assert_eq!(recompute_all(f.db.conn()).unwrap(), 0);
    assert_eq!(apply_to_batch(f.db.conn(), batch).unwrap(), 0);
    assert!(!apply_to_transaction(f.db.conn(), id).unwrap());
    assert_eq!(f.name(id).as_deref(), Some("Coffee"));

    clear_user_name(&mut f.db, id).unwrap();
    assert_eq!(f.shown(id), (Some("ACME COFFEE".into()), None));

    let missing = TransactionId::new(999);
    assert!(matches!(
        set_user_name(&mut f.db, missing, &name("x")),
        Err(Error::TransactionNotFound(_))
    ));
    assert!(matches!(
        clear_user_name(&mut f.db, missing),
        Err(Error::TransactionNotFound(_))
    ));
    assert!(matches!(
        apply_to_transaction(f.db.conn(), missing),
        Err(Error::TransactionNotFound(_))
    ));
}

#[test]
fn store_round_trips_and_refuses_duplicates() {
    let mut f = Fixture::new();
    let (exact, _) = add(
        &mut f.db,
        &definition(AliasKind::Exact, "Acme Coffee", "Acme"),
    )
    .unwrap();
    let (regex, _) = add(&mut f.db, &definition(AliasKind::Regex, "^acme", "Acme")).unwrap();
    assert!(regex > exact);
    assert_eq!(
        get(&f.db, exact).unwrap().definition(),
        &definition(AliasKind::Exact, "Acme Coffee", "Acme")
    );
    let ids: Vec<_> = list(&f.db).unwrap().iter().map(MerchantAlias::id).collect();
    assert_eq!(ids, [exact, regex]);

    // The same match, ignoring case and repeated whitespace.
    assert!(matches!(
        add(&mut f.db, &definition(AliasKind::Exact, "ACME  COFFEE", "Other")),
        Err(Error::Duplicate(id)) if id == exact
    ));
    assert!(matches!(
        add(&mut f.db, &definition(AliasKind::Regex, "^acme", "Other")),
        Err(Error::Duplicate(id)) if id == regex
    ));
    assert!(matches!(
        edit(&mut f.db, regex, &definition(AliasKind::Exact, "acme coffee", "Other")),
        Err(Error::Duplicate(id)) if id == exact
    ));
    // Another kind, or the alias itself, is fine.
    add(
        &mut f.db,
        &definition(AliasKind::Prefix, "Acme Coffee", "Acme"),
    )
    .unwrap();
    edit(
        &mut f.db,
        exact,
        &definition(AliasKind::Exact, "ACME COFFEE", "Acme"),
    )
    .unwrap();

    let missing = MerchantAliasId::new(999);
    assert!(matches!(get(&f.db, missing), Err(Error::NotFound(_))));
    assert!(matches!(
        edit(&mut f.db, missing, &definition(AliasKind::Exact, "x", "x")),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        delete(&mut f.db, missing),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn a_stored_alias_that_no_longer_validates_is_named_by_id() {
    let f = Fixture::new();
    f.db.conn()
        .execute(
            "INSERT INTO merchant_alias (match_kind, pattern, display_name) \
             VALUES ('regex', 'SECRET(', 'Acme')",
            [],
        )
        .unwrap();
    let err = list(&f.db).unwrap_err();
    assert!(matches!(
        err,
        Error::Invalid {
            problem: InvalidAlias::Definition(AliasError::Pattern(_)),
            ..
        }
    ));
    assert!(!err.to_string().contains("SECRET"), "{err}");
    assert!(recompute_all(f.db.conn()).is_err());
}

#[test]
fn opening_an_older_database_names_existing_rows() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("coffer.db");
    {
        let db = crate::db::open_at_version(&path, SCHEMA_VERSION - 1).unwrap();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO account (name, kind, currency) VALUES ('Current', 'current', 'GBP')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO import_batch (account_id, source_file, format) \
             VALUES (1, 'statement.csv', 'csv')",
            [],
        )
        .unwrap();
        for (i, (merchant, description)) in [
            (Some("SQ *ACME COFFEE"), "x"),
            (None, "CONTACTLESS ACME BAKERY 12/11"),
            (None, ""),
        ]
        .into_iter()
        .enumerate()
        {
            conn.execute(
                "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, \
                 currency, merchant, description_raw, description_normalized, fingerprint, \
                 origin) VALUES (1, 1, '2026-11-02', -450, 'GBP', ?1, ?2, ?2, ?3, 'imported')",
                params![merchant, description, format!("fp-{i}")],
            )
            .unwrap();
        }
    }

    let db = Database::open(&path).unwrap();
    let mut statement = db
        .conn()
        .prepare("SELECT merchant_display FROM \"transaction\" ORDER BY id")
        .unwrap();
    let names: Vec<Option<String>> = statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        names,
        [Some("ACME COFFEE".into()), Some("ACME BAKERY".into()), None]
    );
}
