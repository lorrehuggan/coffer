//! Tests for exports against a real database.

use chrono::NaiveTime;
use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{AccountKind, NewAccount};
use crate::exclusion::{self, ExclusionReason};
use crate::manual::{self, ManualTransaction};
use crate::note;
use crate::split::{self, SplitLine};

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn key(key: &str) -> CategoryKey {
    CategoryKey::new(key).unwrap()
}

fn tag_name(name: &str) -> TagName {
    TagName::new(name).unwrap()
}

const PRIVATE_TAG: &str = "Therapy";
const USER_NOTE: &str = "user note text";
const LINE_NOTE: &str = "line note text";
const EXCLUDED_NOTE: &str = "excluded note text";
const BANK_NOTE: &str = "bank note text";

struct Fixture {
    db: Database,
    dir: TempDir,
    current: AccountId,
    card: AccountId,
    /// On the 2nd, −12.34 in the current account, `food.eating_out`, tagged
    /// "Holiday" and the private tag, with a user note and a bank note.
    lunch: TransactionId,
    /// On the 3rd, −50.00 in the current account, split 30.00
    /// `food.groceries` (with a line note) and 20.00 `home.household`.
    shop: TransactionId,
    /// On the 5th, −9.99 on the card, uncategorised, excluded as `other`
    /// with a note; description has a comma and a quote.
    refund: TransactionId,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let current = account::create(
            &mut db,
            &NewAccount::new("Current", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        let card = account::create(
            &mut db,
            &NewAccount::new("Card", AccountKind::CreditCard, Currency::GBP),
        )
        .unwrap();

        let mut entry = ManualTransaction::new(current, day(2), gbp(-1_234), "LUNCH PLACE");
        entry.time = Some(NaiveTime::from_hms_opt(12, 30, 0).unwrap());
        entry.merchant = Some("Lunch Place".into());
        entry.category = Some(key("food.eating_out"));
        let lunch = manual::create(&mut db, &entry).unwrap();
        let holiday = tag::create(&mut db, &tag_name("Holiday"), false).unwrap();
        let private = tag::create(&mut db, &tag_name(PRIVATE_TAG), true).unwrap();
        tag::add_to_transaction(&mut db, lunch, holiday).unwrap();
        tag::add_to_transaction(&mut db, lunch, private).unwrap();
        note::set(&mut db, lunch, USER_NOTE).unwrap();
        // A manual row's note is editable; an imported one's comes from the
        // bank's file.
        db.conn()
            .execute(
                "UPDATE \"transaction\" SET note = ?1 WHERE id = ?2",
                params![BANK_NOTE, lunch],
            )
            .unwrap();

        let shop = manual::create(
            &mut db,
            &ManualTransaction::new(current, day(3), gbp(-5_000), "SUPERMARKET"),
        )
        .unwrap();
        let mut groceries = SplitLine::new(gbp(-3_000), Some(key("food.groceries")));
        groceries.user_note = Some(LINE_NOTE.into());
        groceries.my_share = Some(gbp(-1_500));
        groceries.share_label = Some("Flatmate".into());
        split::set(
            &mut db,
            shop,
            &[
                groceries,
                SplitLine::new(gbp(-2_000), Some(key("home.household"))),
            ],
        )
        .unwrap();

        let refund = manual::create(
            &mut db,
            &ManualTransaction::new(card, day(5), gbp(-999), "SHOP, \"THE\" LTD"),
        )
        .unwrap();
        exclusion::exclude(&mut db, refund, ExclusionReason::Other, Some(EXCLUDED_NOTE)).unwrap();

        Fixture {
            db,
            dir,
            current,
            card,
            lunch,
            shop,
            refund,
        }
    }

    fn rows(&self, options: &Options) -> Vec<ExportRow> {
        rows(&self.db, options).unwrap()
    }

    fn ids(&self, filter: Filter) -> Vec<(TransactionId, Option<i64>)> {
        self.rows(&Options {
            filter,
            ..Options::default()
        })
        .iter()
        .map(|row| (row.transaction_id, row.line_no))
        .collect()
    }

    fn text(&self, format: Format, options: &Options) -> String {
        let mut out = Vec::new();
        write(&self.db, format, options, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }
}

fn with_everything() -> Options {
    Options {
        include_notes: true,
        include_private_tags: true,
        ..Options::default()
    }
}

#[test]
fn columns_depend_only_on_the_opt_ins() {
    let default = columns(&Options::default());
    assert_eq!(default.len(), 41);
    assert_eq!(default[..3], ["transaction_id", "split_id", "line_no"]);
    assert_eq!(default.last(), Some(&"personal_amount_minor"));
    for left_out in [
        "share_label",
        "user_note",
        "line_note",
        "excluded_note",
        "bank_note",
        "private_tags",
    ] {
        assert!(!default.contains(&left_out), "{left_out}");
    }

    let notes = columns(&Options {
        include_notes: true,
        ..Options::default()
    });
    assert_eq!(
        notes[41..],
        ["user_note", "line_note", "excluded_note", "bank_note"]
    );
    let private = columns(&Options {
        include_private_tags: true,
        ..Options::default()
    });
    assert_eq!(private[41..], ["private_tags"]);
    assert_eq!(
        columns(&with_everything())[41..],
        [
            "user_note",
            "line_note",
            "excluded_note",
            "bank_note",
            "private_tags"
        ]
    );
}

#[test]
fn split_lines_are_rows_that_keep_their_parent() {
    let fixture = Fixture::new();
    let rows = fixture.rows(&Options::default());
    assert_eq!(
        rows.iter()
            .map(|row| (row.transaction_id, row.split_id.is_some(), row.line_no))
            .collect::<Vec<_>>(),
        [
            (fixture.lunch, false, None),
            (fixture.shop, true, Some(1)),
            (fixture.shop, true, Some(2)),
            (fixture.refund, false, None),
        ]
    );

    let lunch = &rows[0];
    assert_eq!(lunch.date, day(2));
    assert_eq!(lunch.time.as_deref(), Some("12:30:00"));
    assert_eq!(lunch.account_id, fixture.current);
    assert_eq!(lunch.account_name, "Current");
    assert_eq!(lunch.amount, gbp(-1_234));
    assert_eq!(lunch.my_amount, gbp(-1_234));
    assert_eq!(lunch.transaction_amount, gbp(-1_234));
    assert_eq!(lunch.merchant.as_deref(), Some("Lunch Place"));
    assert_eq!(lunch.description, "LUNCH PLACE");
    assert_eq!(lunch.category_key, Some(key("food.eating_out")));
    assert_eq!(lunch.category_source.as_deref(), Some("user"));
    assert_eq!(lunch.tags, ["Holiday"]);
    assert_eq!(lunch.origin, "manual");
    assert_eq!(lunch.notes, None);
    assert_eq!(lunch.private_tags, None);

    let (groceries, household) = (&rows[1], &rows[2]);
    assert_eq!(groceries.amount, gbp(-3_000));
    assert_eq!(groceries.my_amount, gbp(-1_500));
    assert_eq!(groceries.transaction_amount, gbp(-5_000));
    assert_eq!(groceries.category_key, Some(key("food.groceries")));
    assert_eq!(groceries.category_source, None);
    assert_eq!(household.amount, gbp(-2_000));
    assert_eq!(household.category_key, Some(key("home.household")));
    assert_eq!(household.description, "SUPERMARKET");

    let refund = &rows[3];
    assert_eq!(refund.account_id, fixture.card);
    assert_eq!(refund.excluded_reason.as_deref(), Some("other"));
    assert!(refund.excluded_at.is_some());
    assert_eq!(refund.category_key, None);
}

#[test]
fn notes_and_private_tags_only_when_asked_for() {
    let fixture = Fixture::new();
    let secrets = [
        PRIVATE_TAG,
        USER_NOTE,
        LINE_NOTE,
        EXCLUDED_NOTE,
        BANK_NOTE,
        "Flatmate",
    ];
    for format in [Format::Csv, Format::Json] {
        let text = fixture.text(format, &Options::default());
        for secret in secrets {
            assert!(!text.contains(secret), "{format:?} has {secret:?}");
        }
        assert!(text.contains("Holiday"));
    }

    let rows = fixture.rows(&with_everything());
    assert_eq!(rows[0].tags, ["Holiday"], "tags stay public-only");
    assert_eq!(
        rows[0].private_tags.as_deref(),
        Some(&[PRIVATE_TAG.into()][..])
    );
    assert_eq!(
        rows[0].notes,
        Some(Notes {
            user_note: Some(USER_NOTE.into()),
            line_note: None,
            excluded_note: None,
            bank_note: Some(BANK_NOTE.into()),
        })
    );
    assert_eq!(
        rows[1].notes.as_ref().unwrap().line_note.as_deref(),
        Some(LINE_NOTE)
    );
    assert_eq!(rows[2].notes.as_ref().unwrap().line_note, None);
    assert_eq!(
        rows[3].notes.as_ref().unwrap().excluded_note.as_deref(),
        Some(EXCLUDED_NOTE)
    );
    for format in [Format::Csv, Format::Json] {
        let text = fixture.text(format, &with_everything());
        for secret in &secrets[..5] {
            assert!(text.contains(secret), "{format:?} lacks {secret:?}");
        }
        assert!(!text.contains("Flatmate"), "share labels never leave");
    }

    // Notes alone don't bring private tags, and the other way round.
    let notes_only = fixture.text(
        Format::Json,
        &Options {
            include_notes: true,
            ..Options::default()
        },
    );
    assert!(notes_only.contains(USER_NOTE) && !notes_only.contains(PRIVATE_TAG));
    let tags_only = fixture.text(
        Format::Json,
        &Options {
            include_private_tags: true,
            ..Options::default()
        },
    );
    assert!(tags_only.contains(PRIVATE_TAG) && !tags_only.contains(USER_NOTE));
}

#[test]
fn csv_has_a_header_and_quotes_what_needs_it() {
    let fixture = Fixture::new();
    let text = fixture.text(Format::Csv, &Options::default());
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0], columns(&Options::default()).join(","));
    assert!(lines[1].starts_with(&format!("{},,,2026-11-02,12:30:00,", fixture.lunch)));
    assert!(lines[1].contains(",-12.34,-1234,-12.34,-1234,-12.34,-1234,,,,"));
    assert!(lines[1].contains(",Holiday,false,"));
    assert!(lines[2].starts_with(&format!("{},", fixture.shop)));
    assert!(lines[2].contains(",-30.00,-3000,-15.00,-1500,-50.00,-5000,"));
    assert!(lines[4].contains(",\"SHOP, \"\"THE\"\" LTD\","));
    assert_eq!(csv_field("plain"), "plain");
    assert_eq!(csv_field("a\nb"), "\"a\nb\"");
}

#[test]
fn json_has_the_schema_and_integers_only() {
    let fixture = Fixture::new();
    let text = fixture.text(Format::Json, &with_everything());
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["schema"], "coffer.export");
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["includes"]["notes"], true);
    assert_eq!(json["includes"]["private_tags"], true);
    let rows = json["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 4);

    let lunch = rows[0].as_object().unwrap();
    let names: Vec<&str> = lunch.keys().map(String::as_str).collect();
    let mut expected = columns(&with_everything());
    expected.sort_unstable();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, expected);
    assert_eq!(lunch["transaction_id"], fixture.lunch.get());
    assert!(lunch["split_id"].is_null());
    assert_eq!(lunch["amount"], "-12.34");
    assert_eq!(lunch["amount_minor"], -1234);
    assert_eq!(lunch["currency"], "GBP");
    assert_eq!(lunch["tags"], serde_json::json!(["Holiday"]));
    assert_eq!(lunch["private_tags"], serde_json::json!([PRIVATE_TAG]));
    assert_eq!(lunch["reimbursable"], false);
    assert_eq!(rows[1]["transaction_id"], fixture.shop.get());
    assert_eq!(rows[1]["line_no"], 1);
    assert_eq!(rows[2]["line_no"], 2);

    // No floats anywhere.
    fn no_floats(value: &serde_json::Value) {
        match value {
            serde_json::Value::Number(number) => assert!(number.is_i64(), "{number}"),
            serde_json::Value::Array(items) => items.iter().for_each(no_floats),
            serde_json::Value::Object(map) => map.values().for_each(no_floats),
            _ => {}
        }
    }
    no_floats(&json);

    // In column order.
    let row = &text[text.find("\"rows\"").unwrap()..];
    let first = row.find("\"transaction_id\"").unwrap();
    let notes = row.find("\"user_note\"").unwrap();
    let last = row.find("\"private_tags\"").unwrap();
    assert!(first < notes && notes < last);

    // Nothing to export is still a valid document.
    let empty = fixture.text(
        Format::Json,
        &Options {
            filter: Filter {
                from: Some(day(20)),
                ..Filter::default()
            },
            ..Options::default()
        },
    );
    let json: serde_json::Value = serde_json::from_str(&empty).unwrap();
    assert_eq!(json["rows"], serde_json::json!([]));
}

#[test]
fn filters_by_date_account_category_and_tag() {
    let fixture = Fixture::new();
    let (lunch, shop, refund) = (fixture.lunch, fixture.shop, fixture.refund);

    assert_eq!(
        fixture.ids(Filter {
            from: Some(day(3)),
            to: Some(day(3)),
            ..Filter::default()
        }),
        [(shop, Some(1)), (shop, Some(2))]
    );
    assert_eq!(
        fixture.ids(Filter {
            to: Some(day(2)),
            ..Filter::default()
        }),
        [(lunch, None)]
    );
    assert_eq!(
        fixture.ids(Filter {
            accounts: vec![fixture.card],
            ..Filter::default()
        }),
        [(refund, None)]
    );
    // A group includes its subcategories; a subcategory is just itself.
    assert_eq!(
        fixture.ids(Filter {
            categories: vec![key("food")],
            ..Filter::default()
        }),
        [(lunch, None), (shop, Some(1))]
    );
    assert_eq!(
        fixture.ids(Filter {
            categories: vec![key("food.groceries"), key("home")],
            ..Filter::default()
        }),
        [(shop, Some(1)), (shop, Some(2))]
    );
    // Tags ignore case; filters combine with AND.
    assert_eq!(
        fixture.ids(Filter {
            tags: vec![tag_name("holiday")],
            ..Filter::default()
        }),
        [(lunch, None)]
    );
    assert_eq!(
        fixture.ids(Filter {
            tags: vec![tag_name("Holiday")],
            accounts: vec![fixture.card],
            ..Filter::default()
        }),
        []
    );
}

#[test]
fn a_bad_filter_is_an_error() {
    let fixture = Fixture::new();
    let error = |filter: Filter, include_private_tags| {
        rows(
            &fixture.db,
            &Options {
                filter,
                include_private_tags,
                ..Options::default()
            },
        )
        .unwrap_err()
    };
    assert!(matches!(
        error(
            Filter {
                from: Some(day(5)),
                to: Some(day(4)),
                ..Filter::default()
            },
            false
        ),
        Error::EmptyRange { .. }
    ));
    assert!(matches!(
        error(
            Filter {
                accounts: vec![AccountId::new(99)],
                ..Filter::default()
            },
            false
        ),
        Error::UnknownAccount(_)
    ));
    assert!(matches!(
        error(
            Filter {
                categories: vec![key("nope")],
                ..Filter::default()
            },
            false
        ),
        Error::UnknownCategory(_)
    ));
    assert!(matches!(
        error(
            Filter {
                tags: vec![tag_name("Nope")],
                ..Filter::default()
            },
            false
        ),
        Error::UnknownTag
    ));
    // Filtering by a private tag would say which rows carry it.
    let private = Filter {
        tags: vec![tag_name(PRIVATE_TAG)],
        ..Filter::default()
    };
    let refused = error(private.clone(), false);
    assert!(matches!(refused, Error::PrivateTag));
    assert!(!refused.to_string().contains(PRIVATE_TAG));
    assert_eq!(
        rows(
            &fixture.db,
            &Options {
                filter: private,
                include_private_tags: true,
                ..Options::default()
            }
        )
        .unwrap()
        .len(),
        1
    );
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn a_file_is_private_and_not_overwritten_unless_asked() {
    let fixture = Fixture::new();
    let path = fixture.dir.path().join("export.csv");
    let options = Options::default();

    assert_eq!(
        to_file(&fixture.db, Format::Csv, &options, &path, false).unwrap(),
        4
    );
    #[cfg(unix)]
    assert_eq!(mode(&path), 0o600);
    let first = fs::read_to_string(&path).unwrap();
    assert_eq!(first, fixture.text(Format::Csv, &options));

    assert!(matches!(
        to_file(&fixture.db, Format::Json, &options, &path, false),
        Err(Error::FileExists(existing)) if existing == path
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), first);

    // Overwriting replaces it whole, and a looser file ends up private.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    }
    to_file(&fixture.db, Format::Json, &options, &path, true).unwrap();
    assert!(fs::read_to_string(&path).unwrap().starts_with('{'));
    #[cfg(unix)]
    assert_eq!(mode(&path), 0o600);
    let leftovers: Vec<_> = fs::read_dir(fixture.dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("coffer-export"))
        .collect();
    assert_eq!(leftovers, Vec::<String>::new());

    // A filter error leaves no file behind.
    let missing = fixture.dir.path().join("missing.csv");
    let bad = Options {
        filter: Filter {
            categories: vec![key("nope")],
            ..Filter::default()
        },
        ..Options::default()
    };
    assert!(to_file(&fixture.db, Format::Csv, &bad, &missing, false).is_err());
    assert!(!missing.exists());
}
