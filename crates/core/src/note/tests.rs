//! Setting and clearing the user's note, against a real database, and that
//! the imported note is never touched.

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

    /// An imported row with the bank's note `Rent for May`, and its mirrored
    /// row on a pot, with the same note.
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
                 currency, description_raw, description_normalized, fingerprint, note, origin) \
                 VALUES (?1, ?2, '2026-11-02', ?3, 'GBP', 'Pot', 'POT', ?4, 'Rent for May', ?4)",
                params![account, batch, amount, origin],
            )
            .unwrap();
            TransactionId::new(conn.last_insert_rowid())
        };
        let imported = insert(self.account, "imported", -1_000);
        let mirrored = insert(pot, "mirrored", 1_000);
        (imported, mirrored)
    }

    /// `(note, user_note)` as stored.
    fn notes(&self, transaction: TransactionId) -> (Option<String>, Option<String>) {
        self.db
            .conn()
            .query_row(
                "SELECT note, user_note FROM \"transaction\" WHERE id = ?1",
                [transaction],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    }
}

fn note_of(text: &str) -> UserNote {
    UserNote::new(text).unwrap().unwrap()
}

// Notes ------------------------------------------------------------------------

#[test]
fn notes_are_trimmed_and_keep_their_inside() {
    assert_eq!(
        note_of("  Claim on expenses\n\nReceipt in email  \n").as_str(),
        "Claim on expenses\n\nReceipt in email"
    );
}

#[test]
fn blank_text_is_no_note() {
    for text in ["", "   ", "\n\t", "\u{a0}"] {
        assert_eq!(UserNote::new(text), Ok(None), "{text:?}");
    }
}

#[test]
fn notes_are_at_most_2000_characters() {
    // Characters, not bytes: `é` is two bytes.
    let longest = "é".repeat(MAX_USER_NOTE_LEN);
    assert_eq!(note_of(&longest).as_str(), longest);
    assert_eq!(
        UserNote::new(&"x".repeat(MAX_USER_NOTE_LEN + 1)),
        Err(NoteError::TooLong {
            max: MAX_USER_NOTE_LEN
        })
    );
    // Trimmed first.
    assert!(
        UserNote::new(&format!("\n{}\n", "x".repeat(MAX_USER_NOTE_LEN)))
            .unwrap()
            .is_some()
    );
}

// Setting and clearing ---------------------------------------------------------

#[test]
fn set_get_and_clear() {
    let mut f = Fixture::new();
    let transaction = f.manual();
    assert_eq!(get(&f.db, transaction).unwrap(), None);

    let stored = set(&mut f.db, transaction, "  with Sam ").unwrap();
    assert_eq!(stored, Some(note_of("with Sam")));
    assert_eq!(
        get(&f.db, transaction).unwrap().as_deref(),
        Some("with Sam")
    );

    set(&mut f.db, transaction, "Paid back").unwrap();
    assert_eq!(
        get(&f.db, transaction).unwrap().as_deref(),
        Some("Paid back")
    );

    clear(&mut f.db, transaction).unwrap();
    assert_eq!(f.notes(transaction), (None, None));
}

#[test]
fn blank_text_clears_the_note_to_null() {
    let mut f = Fixture::new();
    let transaction = f.manual();
    set(&mut f.db, transaction, "with Sam").unwrap();

    assert_eq!(set(&mut f.db, transaction, " \n ").unwrap(), None);
    // NULL, not an empty string.
    let is_null: bool =
        f.db.conn()
            .query_row(
                "SELECT user_note IS NULL FROM \"transaction\" WHERE id = ?1",
                [transaction],
                |row| row.get(0),
            )
            .unwrap();
    assert!(is_null);
}

#[test]
fn a_note_too_long_changes_nothing() {
    let mut f = Fixture::new();
    let transaction = f.manual();
    set(&mut f.db, transaction, "with Sam").unwrap();

    let err = set(&mut f.db, transaction, &"Secret ".repeat(MAX_USER_NOTE_LEN)).unwrap_err();
    assert!(matches!(err, Error::Note(NoteError::TooLong { .. })));
    assert!(!err.to_string().contains("Secret"), "{err}");
    assert_eq!(
        get(&f.db, transaction).unwrap().as_deref(),
        Some("with Sam")
    );
}

#[test]
fn a_missing_transaction_is_an_error() {
    let mut f = Fixture::new();
    let missing = TransactionId::new(99);
    assert!(matches!(
        set(&mut f.db, missing, "note"),
        Err(Error::TransactionNotFound(id)) if id == missing
    ));
    assert!(matches!(
        clear(&mut f.db, missing),
        Err(Error::TransactionNotFound(_))
    ));
    assert!(matches!(
        get(&f.db, missing),
        Err(Error::TransactionNotFound(_))
    ));
}

#[test]
fn debug_never_shows_the_note() {
    assert!(!format!("{:?}", note_of("Secret clinic")).contains("Secret"));
}

// The imported note --------------------------------------------------------------

#[test]
fn the_user_note_works_on_every_origin_and_leaves_the_imported_note() {
    let mut f = Fixture::new();
    let (imported, mirrored) = f.imported_and_mirrored();
    let manual = f.manual();

    for transaction in [imported, mirrored] {
        set(&mut f.db, transaction, "Split with Sam").unwrap();
        assert_eq!(
            f.notes(transaction),
            (Some("Rent for May".into()), Some("Split with Sam".into()))
        );
        clear(&mut f.db, transaction).unwrap();
        assert_eq!(f.notes(transaction), (Some("Rent for May".into()), None));
    }

    set(&mut f.db, manual, "Cash for the window cleaner").unwrap();
    assert_eq!(
        f.notes(manual),
        (None, Some("Cash for the window cleaner".into()))
    );
}

#[test]
fn the_imported_note_is_read_only() {
    let mut f = Fixture::new();
    let (imported, mirrored) = f.imported_and_mirrored();

    for transaction in [imported, mirrored] {
        for sql in [
            "UPDATE \"transaction\" SET note = 'edited' WHERE id = ?1",
            "UPDATE \"transaction\" SET note = NULL WHERE id = ?1",
            "UPDATE \"transaction\" SET note = 'edited', user_note = 'mine' WHERE id = ?1",
        ] {
            let err = f.db.conn().execute(sql, [transaction]).unwrap_err();
            assert_eq!(
                err.sqlite_error_code(),
                Some(ErrorCode::ConstraintViolation),
                "{sql}"
            );
            assert!(
                err.to_string()
                    .contains("imported source fields are read-only")
            );
        }
        assert_eq!(f.notes(transaction), (Some("Rent for May".into()), None));
    }
}
