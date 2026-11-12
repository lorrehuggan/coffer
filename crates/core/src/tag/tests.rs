//! Tag names, the tag store and links to transactions, against a real
//! database.

use rusqlite::ErrorCode;
use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount};
use crate::id::AccountId;
use crate::money::Currency;

struct Fixture {
    db: Database,
    _dir: TempDir,
    account: AccountId,
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
        }
    }

    fn tag(&mut self, name: &str, private: bool) -> TagId {
        create(&mut self.db, &name_of(name), private).unwrap()
    }

    /// A manual transaction of -5.00 GBP.
    fn manual(&self) -> TransactionId {
        self.db
            .conn()
            .execute(
                "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
                 description_raw, description_normalized, origin) \
                 VALUES (?1, '2026-11-03', -500, 'GBP', 'Cash', 'CASH', 'manual')",
                [self.account],
            )
            .unwrap();
        TransactionId::new(self.db.conn().last_insert_rowid())
    }

    /// An imported row and, on a pot of the account, its mirrored row.
    fn imported_and_mirrored(&mut self) -> (TransactionId, TransactionId) {
        let mut pot = NewAccount::new("Rainy day", AccountKind::Pot, Currency::GBP);
        pot.parent = Some(self.account);
        let pot = account::create(&mut self.db, &pot).unwrap();
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO import_batch (account_id, source_file, format) \
             VALUES (?1, 'statement.csv', 'csv')",
            [self.account],
        )
        .unwrap();
        let batch = conn.last_insert_rowid();
        let insert = |account: AccountId, origin: &str, amount: i64| {
            conn.execute(
                "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, \
                 currency, description_raw, description_normalized, fingerprint, origin) \
                 VALUES (?1, ?2, '2026-11-02', ?3, 'GBP', 'Pot', 'POT', ?4, ?4)",
                params![account, batch, amount, origin],
            )
            .unwrap();
            TransactionId::new(conn.last_insert_rowid())
        };
        let imported = insert(self.account, "imported", -1_000);
        let mirrored = insert(pot, "mirrored", 1_000);
        (imported, mirrored)
    }

    fn links(&self) -> i64 {
        self.db
            .conn()
            .query_row("SELECT COUNT(*) FROM transaction_tag", [], |row| row.get(0))
            .unwrap()
    }
}

fn name_of(text: &str) -> TagName {
    TagName::new(text).unwrap()
}

fn names(tags: &[Tag]) -> Vec<&str> {
    tags.iter().map(|tag| tag.name.as_str()).collect()
}

#[track_caller]
fn assert_refused<T: fmt::Debug>(result: rusqlite::Result<T>) {
    match result {
        Err(err) if err.sqlite_error_code() == Some(ErrorCode::ConstraintViolation) => {}
        other => panic!("expected a constraint violation, got {other:?}"),
    }
}

// Names ------------------------------------------------------------------------

#[test]
fn names_are_trimmed_and_keep_their_case() {
    assert_eq!(name_of("  Gift Aid\t").as_str(), "Gift Aid");
    assert_eq!(name_of("work").as_str(), "work");
    assert_ne!(name_of("Work"), name_of("work"));
}

#[test]
fn blank_names_are_refused() {
    for text in ["", " ", "\t\n", "\u{a0}"] {
        assert_eq!(TagName::new(text), Err(TagNameError::Blank), "{text:?}");
    }
}

#[test]
fn names_are_at_most_64_characters() {
    // Characters, not bytes: `é` is two bytes.
    let longest = "é".repeat(MAX_TAG_NAME_LEN);
    assert_eq!(name_of(&longest).as_str(), longest);
    assert_eq!(
        TagName::new(&"x".repeat(MAX_TAG_NAME_LEN + 1)),
        Err(TagNameError::TooLong {
            max: MAX_TAG_NAME_LEN
        })
    );
    // Trimmed first.
    assert!(TagName::new(&format!(" {} ", "x".repeat(MAX_TAG_NAME_LEN))).is_ok());
}

#[test]
fn control_characters_are_refused() {
    for text in ["two\nlines", "tab\there", "nul\0", "bell\u{7}", "c1\u{85}x"] {
        assert_eq!(
            TagName::new(text),
            Err(TagNameError::ControlCharacter),
            "{text:?}"
        );
    }
}

#[test]
fn same_name_ignores_case_across_scripts() {
    for (a, b) in [
        ("Work", "WORK"),
        ("Café", "CAFÉ"),
        ("Straße", "STRASSE"),
        ("ΟΔΟΣ", "οδος"),
    ] {
        assert!(name_of(a).same_name(&name_of(b)), "{a} {b}");
    }
    assert!(!name_of("work").same_name(&name_of("works")));
}

#[test]
fn debug_and_errors_never_show_the_name() {
    let name = name_of("Secret Clinic");
    assert!(!format!("{name:?}").contains("Secret"));
    for err in [
        TagNameError::Blank,
        TagNameError::TooLong { max: 64 },
        TagNameError::ControlCharacter,
    ] {
        assert!(!err.to_string().contains("Secret"));
    }
    let mut f = Fixture::new();
    f.tag("Secret Clinic", true);
    let err = create(&mut f.db, &name_of("secret clinic"), false).unwrap_err();
    assert!(!err.to_string().to_lowercase().contains("secret"), "{err}");
    assert!(!format!("{err:?}").to_lowercase().contains("secret"));
}

// The store --------------------------------------------------------------------

#[test]
fn a_tag_round_trips() {
    let mut f = Fixture::new();
    let work = f.tag("Work", false);
    let health = f.tag("Health", true);

    assert_eq!(
        get(&f.db, work).unwrap(),
        Tag {
            id: work,
            name: name_of("Work"),
            private: false,
        }
    );
    assert!(get(&f.db, health).unwrap().private);
    assert!(matches!(
        get(&f.db, TagId::new(99)),
        Err(Error::NotFound(id)) if id == TagId::new(99)
    ));
}

#[test]
fn list_orders_by_name_ignoring_case() {
    let mut f = Fixture::new();
    f.tag("travel", false);
    f.tag("Gift aid", true);
    f.tag("work", false);
    f.tag("Bills", false);
    assert_eq!(
        names(&list(&f.db).unwrap()),
        ["Bills", "Gift aid", "travel", "work"]
    );
}

#[test]
fn names_are_unique_ignoring_case() {
    let mut f = Fixture::new();
    let work = f.tag("Work", false);
    let cafe = f.tag("Café", false);

    for (name, taken) in [
        ("work", work),
        ("WORK", work),
        ("CAFÉ", cafe),
        (" café ", cafe),
    ] {
        assert!(
            matches!(
                create(&mut f.db, &name_of(name), true),
                Err(Error::NameTaken(id)) if id == taken
            ),
            "{name}"
        );
    }
    assert_eq!(list(&f.db).unwrap().len(), 2);

    let found = find(&f.db, &name_of("WORK")).unwrap().unwrap();
    assert_eq!(found.id, work);
    assert_eq!(found.name.as_str(), "Work");
    assert_eq!(find(&f.db, &name_of("Home")).unwrap(), None);
}

#[test]
fn the_schema_refuses_a_name_taken_in_another_case() {
    let mut f = Fixture::new();
    f.tag("Work", false);
    // The backstop for writes that skip core: `NOCASE` folds ASCII.
    assert_refused(
        f.db.conn()
            .execute("INSERT INTO tag (name) VALUES ('WORK')", []),
    );
}

#[test]
fn rename_keeps_names_unique() {
    let mut f = Fixture::new();
    let work = f.tag("Work", false);
    let home = f.tag("Home", false);

    // Its own name in another case is fine.
    rename(&mut f.db, work, &name_of("WORK")).unwrap();
    assert_eq!(get(&f.db, work).unwrap().name.as_str(), "WORK");
    rename(&mut f.db, work, &name_of("Office")).unwrap();
    assert_eq!(get(&f.db, work).unwrap().name.as_str(), "Office");

    assert!(matches!(
        rename(&mut f.db, work, &name_of("home")),
        Err(Error::NameTaken(id)) if id == home
    ));
    assert_eq!(get(&f.db, work).unwrap().name.as_str(), "Office");
    assert!(matches!(
        rename(&mut f.db, TagId::new(99), &name_of("New")),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn private_can_be_set_and_cleared() {
    let mut f = Fixture::new();
    let tag = f.tag("Health", false);
    set_private(&mut f.db, tag, true).unwrap();
    assert!(get(&f.db, tag).unwrap().private);
    set_private(&mut f.db, tag, false).unwrap();
    assert!(!get(&f.db, tag).unwrap().private);
    assert!(matches!(
        set_private(&mut f.db, TagId::new(99), true),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn delete_removes_the_tag_and_its_links_only() {
    let mut f = Fixture::new();
    let work = f.tag("Work", false);
    let home = f.tag("Home", false);
    let (a, b) = (f.manual(), f.manual());
    for (transaction, tag) in [(a, work), (b, work), (a, home)] {
        add_to_transaction(&mut f.db, transaction, tag).unwrap();
    }

    assert_eq!(delete(&mut f.db, work).unwrap(), 2);
    assert!(matches!(get(&f.db, work), Err(Error::NotFound(_))));
    assert_eq!(names(&tags_of(&f.db, a).unwrap()), ["Home"]);
    assert!(tags_of(&f.db, b).unwrap().is_empty());
    assert_eq!(f.links(), 1);
    // The transactions stay.
    assert!(tags_of(&f.db, a).is_ok());

    assert!(matches!(delete(&mut f.db, work), Err(Error::NotFound(_))));
    // An unused tag has no links to remove.
    assert_eq!(delete(&mut f.db, home).unwrap(), 1);
    let unused = f.tag("Unused", false);
    assert_eq!(delete(&mut f.db, unused).unwrap(), 0);
}

// Links ------------------------------------------------------------------------

#[test]
fn a_transaction_has_several_tags() {
    let mut f = Fixture::new();
    let work = f.tag("work", false);
    let travel = f.tag("Travel", false);
    let other = f.manual();
    let transaction = f.manual();

    assert!(add_to_transaction(&mut f.db, transaction, work).unwrap());
    assert!(add_to_transaction(&mut f.db, transaction, travel).unwrap());
    assert_eq!(
        names(&tags_of(&f.db, transaction).unwrap()),
        ["Travel", "work"]
    );
    assert!(tags_of(&f.db, other).unwrap().is_empty());
}

#[test]
fn adding_a_tag_twice_changes_nothing() {
    let mut f = Fixture::new();
    let work = f.tag("Work", false);
    let transaction = f.manual();

    assert!(add_to_transaction(&mut f.db, transaction, work).unwrap());
    assert!(!add_to_transaction(&mut f.db, transaction, work).unwrap());
    assert_eq!(f.links(), 1);
}

#[test]
fn removing_a_link_keeps_the_tag() {
    let mut f = Fixture::new();
    let work = f.tag("Work", false);
    let transaction = f.manual();
    add_to_transaction(&mut f.db, transaction, work).unwrap();

    assert!(remove_from_transaction(&mut f.db, transaction, work).unwrap());
    assert!(tags_of(&f.db, transaction).unwrap().is_empty());
    assert_eq!(get(&f.db, work).unwrap().name.as_str(), "Work");
    assert_eq!(names(&list(&f.db).unwrap()), ["Work"]);

    assert!(!remove_from_transaction(&mut f.db, transaction, work).unwrap());
}

#[test]
fn links_need_a_tag_and_a_transaction() {
    let mut f = Fixture::new();
    let work = f.tag("Work", false);
    let transaction = f.manual();
    let missing = TransactionId::new(99);

    assert!(matches!(
        add_to_transaction(&mut f.db, missing, work),
        Err(Error::TransactionNotFound(id)) if id == missing
    ));
    assert!(matches!(
        add_to_transaction(&mut f.db, transaction, TagId::new(99)),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        remove_from_transaction(&mut f.db, missing, work),
        Err(Error::TransactionNotFound(_))
    ));
    assert!(matches!(
        remove_from_transaction(&mut f.db, transaction, TagId::new(99)),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        tags_of(&f.db, missing),
        Err(Error::TransactionNotFound(_))
    ));
    assert!(matches!(
        public_tags_of(&f.db, missing),
        Err(Error::TransactionNotFound(_))
    ));
    assert_eq!(f.links(), 0);
}

#[test]
fn tags_go_on_rows_of_every_origin() {
    let mut f = Fixture::new();
    let work = f.tag("Work", false);
    let (imported, mirrored) = f.imported_and_mirrored();
    let manual = f.manual();

    for transaction in [imported, mirrored, manual] {
        assert!(add_to_transaction(&mut f.db, transaction, work).unwrap());
        assert_eq!(names(&tags_of(&f.db, transaction).unwrap()), ["Work"]);
        assert!(remove_from_transaction(&mut f.db, transaction, work).unwrap());
    }
}

// Private tags -----------------------------------------------------------------

#[test]
fn private_tags_stay_in_the_app() {
    let mut f = Fixture::new();
    let work = f.tag("Work", false);
    let clinic = f.tag("Clinic", true);
    let travel = f.tag("Travel", false);
    let transaction = f.manual();
    for tag in [work, clinic, travel] {
        add_to_transaction(&mut f.db, transaction, tag).unwrap();
    }

    let all = tags_of(&f.db, transaction).unwrap();
    assert_eq!(names(&all), ["Clinic", "Travel", "Work"]);
    assert_eq!(
        names(&public_tags_of(&f.db, transaction).unwrap()),
        ["Travel", "Work"]
    );
    assert_eq!(names(&without_private(all)), ["Travel", "Work"]);
    assert_eq!(
        names(&without_private(list(&f.db).unwrap())),
        ["Travel", "Work"]
    );

    // Marking a tag private takes it out at once.
    set_private(&mut f.db, travel, true).unwrap();
    assert_eq!(
        names(&public_tags_of(&f.db, transaction).unwrap()),
        ["Work"]
    );
}

#[test]
fn has_repeat_ignores_case() {
    assert!(!has_repeat(&[name_of("a"), name_of("b")]));
    assert!(has_repeat(&[name_of("a"), name_of("b"), name_of("A")]));
    assert!(has_repeat(&[name_of("Straße"), name_of("STRASSE")]));
}
