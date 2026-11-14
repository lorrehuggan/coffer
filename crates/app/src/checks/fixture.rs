//! Sample data for the review queues' snapshots and tests: the D6.4
//! fixture (Paper `8SE-0`, `98K-0`, `9MQ-0`), written into a database
//! through core's own API, so what the screens show comes from the real
//! read model. Every merchant, amount and ID is invented.
//!
//! - **Joint account**: £640.00 on 1 Sep (the starting point), £512.40
//!   on 30 Sep (passes), £18.40 on 9 Nov (fails: Coffer counts −£24.10);
//!   locked through 30 Sep. An imported Ocado on 4 Nov and the same typed
//!   in by hand on 5 Nov explain the −£42.50.
//! - **Savings**: an OFX import with balances on 31 Aug, 30 Sep and
//!   31 Oct, all passing; locked through 30 Sep, so it can be locked
//!   through 31 Oct.
//! - **Current account** (Monzo): one balance, nothing to check yet; two
//!   more possible duplicates (Trainline, Pret A Manger).
//! - **Amex** (no balance) and **Stocks ISA** (valued): not checked.

use chrono::NaiveDate;
use coffer_core::account::{self, AccountKind, EnteredBalance, EnteredSource, NewAccount};
use coffer_core::category::CategoryKey;
use coffer_core::db::Database;
use coffer_core::duplicate::{self, DuplicateWindow};
use coffer_core::id::{AccountId, TransactionId};
use coffer_core::import::{self, Committed};
use coffer_core::manual::{self, ManualTransaction};
use coffer_core::money::{Currency, Money};
use coffer_core::reconciliation;
use coffer_core::statement::{
    DatedBalance, SourceFormat, SourceLocation, Statement, StatementLine,
};

fn date(month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, day).expect("a fixture date")
}

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

/// One imported row: date, amount, bank text, bank ID.
type Row = ((u32, u32), i64, &'static str, Option<&'static str>);

fn import(
    db: &mut Database,
    account: AccountId,
    file: &str,
    format: SourceFormat,
    rows: &[Row],
    balances: &[((u32, u32), i64)],
) {
    let mut statement = Statement::new(format, Currency::GBP);
    for (index, ((month, day), minor, text, id)) in rows.iter().enumerate() {
        let mut line = StatementLine::new(date(*month, *day), gbp(*minor), *text);
        line.external_id = id.map(str::to_owned);
        statement.push(
            SourceLocation::Line(u64::try_from(index).unwrap_or(0) + 2),
            Ok(line),
        );
    }
    for ((month, day), minor) in balances {
        statement
            .push_balance(DatedBalance {
                date: date(*month, *day),
                balance: gbp(*minor),
            })
            .expect("a GBP balance");
    }
    let preview = import::preview(db, account, file, statement).expect("a fixture preview");
    let Committed::Batch(_) =
        import::commit(db, &preview, preview.needs_confirmation()).expect("a fixture import")
    else {
        panic!("the fixture import made no batch");
    };
}

fn manual(
    db: &mut Database,
    account: AccountId,
    (month, day): (u32, u32),
    minor: i64,
    merchant: &str,
    category: Option<&str>,
) -> TransactionId {
    let mut entry = ManualTransaction::new(account, date(month, day), gbp(minor), merchant);
    entry.merchant = Some(merchant.to_owned());
    entry.category = category.map(|key| CategoryKey::new(key).expect("a fixture category"));
    manual::create(db, &entry).expect("a fixture row")
}

fn entered(db: &mut Database, account: AccountId, (month, day): (u32, u32), minor: i64) {
    account::add_balance(
        db,
        account,
        &EnteredBalance {
            date: date(month, day),
            balance: gbp(minor),
            source: EnteredSource::Manual,
            note: None,
        },
    )
    .expect("a fixture balance");
}

fn create(db: &mut Database, name: &str, kind: AccountKind) -> AccountId {
    account::create(db, &NewAccount::new(name, kind, Currency::GBP)).expect("a fixture account")
}

/// The accounts the fixture makes.
pub struct Seeded {
    pub joint: AccountId,
    pub savings: AccountId,
    pub current: AccountId,
}

/// Writes the D6.4 sample accounts, transactions, balances and pairs into
/// `db`, which should be empty.
pub fn seed(db: &mut Database) -> Seeded {
    let joint = create(db, "Joint account", AccountKind::Current);
    let savings = create(db, "Savings", AccountKind::Savings);
    let mut current = NewAccount::new("Current account", AccountKind::Current, Currency::GBP);
    current.preset = Some("uk-monzo".to_owned());
    let current = account::create(db, &current).expect("the fixture's current account");
    create(db, "Amex", AccountKind::CreditCard);
    create(db, "Stocks ISA", AccountKind::Asset);

    // Joint account: −£127.60 in September, −£536.50 since (the copy of
    // Ocado included).
    entered(db, joint, (9, 1), 64_000);
    manual(db, joint, (9, 12), -5_760, "Tesco", Some("food.groceries"));
    manual(db, joint, (9, 20), -7_000, "Thames Water", None);
    entered(db, joint, (9, 30), 51_240);
    import(
        db,
        joint,
        "joint-november.ofx",
        SourceFormat::Ofx,
        &[
            (
                (10, 2),
                -14_800,
                "COUNCIL TAX DD",
                Some("tx_0000Ab12Cd30Aa"),
            ),
            (
                (10, 15),
                -9_640,
                "OCTOPUS ENERGY",
                Some("tx_0000Ab12Cd31Bb"),
            ),
            ((10, 28), -4_510, "TFL TRAVEL CH", Some("tx_0000Ab12Cd32Cc")),
            (
                (11, 4),
                -4_250,
                "OCADO RETAIL LTD HATFIELD GB",
                Some("tx_0000Ab12Cd34Ef"),
            ),
            ((11, 7), -2_000, "BOOTS 1142", Some("tx_0000Ab12Cd35Dd")),
            ((11, 8), -14_200, "WAITROSE 0412", Some("tx_0000Ab12Cd36Ee")),
        ],
        &[],
    );
    manual(
        db,
        joint,
        (11, 5),
        -4_250,
        "OCADO.COM",
        Some("food.groceries"),
    );
    entered(db, joint, (11, 9), 1_840);

    // Savings: everything passes.
    import(
        db,
        savings,
        "savings.ofx",
        SourceFormat::Ofx,
        &[
            ((9, 15), 50_000, "TRANSFER FROM JOINT", Some("sv_0001")),
            ((10, 15), 50_000, "TRANSFER FROM JOINT", Some("sv_0002")),
        ],
        &[((8, 31), 475_000), ((9, 30), 525_000), ((10, 31), 575_000)],
    );

    // Current account: one balance; two more pairs.
    entered(db, current, (9, 1), 107_170);
    import(
        db,
        current,
        "monzo-november.ofx",
        SourceFormat::Ofx,
        &[
            ((11, 3), -3_860, "TRAINLINE.COM", Some("tr_0001")),
            ((11, 6), -420, "PRET LIVERPOOL ST", Some("tr_0002")),
        ],
        &[],
    );
    manual(db, current, (11, 2), -3_860, "Trainline", Some("transport"));
    // Ocado, then Trainline; the CSV import then finds Pret itself.
    duplicate::scan(db, None, DuplicateWindow::DEFAULT).expect("scanning the fixture");
    import(
        db,
        current,
        "monzo-november.csv",
        SourceFormat::Csv,
        &[((11, 6), -420, "PRET A MANGER", None)],
        &[],
    );

    reconciliation::lock(db, joint, date(9, 30)).expect("locking the joint account");
    reconciliation::lock(db, savings, date(9, 30)).expect("locking savings");
    Seeded {
        joint,
        savings,
        current,
    }
}

/// Adds a pair inside a locked period (`9MQ-0`): interest on Savings on
/// 2 Nov, imported and typed in, both counted by a statement balance on
/// 5 Nov that passes, and Savings locked through 5 Nov.
pub fn seed_locked_pair(db: &mut Database, seeded: &Seeded) {
    import(
        db,
        seeded.savings,
        "savings-november.ofx",
        SourceFormat::Ofx,
        &[((11, 2), 1_200, "MONTHLY INTEREST", Some("sv_0003"))],
        &[],
    );
    manual(db, seeded.savings, (11, 2), 1_200, "Interest", None);
    account::add_balance(
        db,
        seeded.savings,
        &EnteredBalance {
            date: date(11, 5),
            balance: gbp(577_400),
            source: EnteredSource::Statement,
            note: None,
        },
    )
    .expect("the statement balance");
    duplicate::scan(db, None, DuplicateWindow::DEFAULT).expect("scanning the fixture");
    reconciliation::lock(db, seeded.savings, date(11, 5)).expect("locking savings");
}
