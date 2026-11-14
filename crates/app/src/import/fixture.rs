//! Sample data for the Import dialog: a synthetic Monzo-style export with
//! the rows Paper's Map step shows (`5ZQ-0`: newest first, two pot
//! transfers, one amount written with a decimal comma), and the accounts
//! and recent imports of the File step (`6J4-0`). Every merchant, ID and
//! amount is made up.

use chrono::NaiveDate;
use coffer_core::account::{Account, AccountKind};
use coffer_core::id::AccountId;
use coffer_core::money::Currency;
use coffer_import::SourceFile;
use coffer_import::preset::Presets;

use super::store::{RecentImport, Setup};

/// The sample file's name.
pub const FILE_NAME: &str = "monzo-november.csv";

/// The sample export, in Monzo's columns.
pub const MONZO_NOVEMBER: &str = "\
Transaction ID,Date,Time,Type,Name,Emoji,Category,Amount,Currency,Local amount,Local currency,Notes and #tags,Address,Receipt,Description,Category split,Money Out,Money In
tx_0000SAMPLE0000000000014,09/11/2026,12:41:09,Card payment,Pret A Manger,,Eating out,-6.45,GBP,-6.45,GBP,,,,PRET A MANGER LONDON GBR,,-6.45,
tx_0000SAMPLE0000000000013,09/11/2026,09:12:30,Card payment,SQ *BLUE BOTTLE COFFEE LDN,,Eating out,-4.20,GBP,-4.20,GBP,,,,SQ *BLUE BOTTLE COFFEE LDN,,-4.20,
tx_0000SAMPLE0000000000012,09/11/2026,08:00:00,Pot transfer,Rainy day,,Transfers,-250.00,GBP,-250.00,GBP,,,,,,-250.00,
tx_0000SAMPLE0000000000011,09/11/2026,07:48:51,Card payment,Transport for London,,Transport,-3.10,GBP,-3.10,GBP,,,,TFL TRAVEL CH LONDON GBR,,-3.10,
tx_0000SAMPLE0000000000010,08/11/2026,18:20:02,Card payment,\"Sainsbury's Local, Clapham High Street\",,Groceries,-18.62,GBP,-18.62,GBP,,,,SAINSBURYS S/MKTS LONDON GBR,,-18.62,
tx_0000SAMPLE0000000000009,08/11/2026,14:03:44,Card payment,ASOS.com,,Shopping,34.00,GBP,34.00,GBP,,,,ASOS.COM REFUND,,,34.00
tx_0000SAMPLE0000000000008,08/11/2026,13:30:10,Card payment,Deliveroo,,Eating out,-27.45,GBP,-27.45,GBP,,,,DELIVEROO LONDON GBR,,-27.45,
tx_0000SAMPLE0000000000007,07/11/2026,21:15:00,Card payment,PAYPAL *STEAMGAMES 4029357733,,Entertainment,\"8,99\",GBP,\"8,99\",GBP,,,,PAYPAL *STEAMGAMES,,,
tx_0000SAMPLE0000000000006,07/11/2026,10:00:00,Pot transfer,Holiday,,Transfers,120.00,GBP,120.00,GBP,,,,,,,120.00
tx_0000SAMPLE0000000000005,07/11/2026,06:30:00,Direct Debit,Octopus Energy,,Bills,-86.40,GBP,-86.40,GBP,,,,OCTOPUS ENERGY,,-86.40,
tx_0000SAMPLE0000000000004,06/11/2026,19:44:12,Card payment,Waitrose,,Groceries,-24.30,GBP,-24.30,GBP,,,,WAITROSE 123 LONDON GBR,,-24.30,
tx_0000SAMPLE0000000000003,06/11/2026,12:02:00,Card payment,Odeon Luxe,,Entertainment,-13.80,GBP,-13.80,GBP,,,,ODEON CINEMAS,,-13.80,
tx_0000SAMPLE0000000000002,05/11/2026,08:55:20,Card payment,Pret A Manger,,Eating out,-7.15,GBP,-7.15,GBP,,,,PRET A MANGER LONDON GBR,,-7.15,
tx_0000SAMPLE0000000000001,01/11/2026,07:58:12,Faster payment,Example Employer Ltd,,Income,2150.00,GBP,2150.00,GBP,,,,EXAMPLE EMPLOYER LTD SALARY NOV,,,2150.00
";

/// The sample export as a file.
pub fn file() -> SourceFile {
    SourceFile::from_bytes(FILE_NAME, MONZO_NOVEMBER)
}

/// The day the sample is set on.
pub fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 9).expect("valid date")
}

fn account(id: i64, name: &str, kind: AccountKind, preset: Option<&str>) -> Account {
    Account {
        id: AccountId::new(id),
        parent: None,
        name: name.to_owned(),
        institution: None,
        kind,
        currency: Currency::GBP,
        opening_balance: None,
        reconciled_through: None,
        closed_on: None,
        preset: preset.map(str::to_owned),
    }
}

/// The File step's sample: the bundled presets, three accounts and the
/// imports Paper lists.
pub fn setup() -> Setup {
    let date = |m, d| NaiveDate::from_ymd_opt(2026, m, d).expect("valid date");
    let recent = |file: &str, account: &str, read_with: &str, date| RecentImport {
        file: file.to_owned(),
        account: account.to_owned(),
        read_with: read_with.to_owned(),
        date,
    };
    Setup {
        presets: Presets::bundled().expect("the bundled presets load"),
        preset_problems: Vec::new(),
        accounts: vec![
            account(1, "Current account", AccountKind::Current, Some("uk-monzo")),
            account(2, "Joint account", AccountKind::Current, Some("uk-monzo")),
            account(3, "Savings", AccountKind::Savings, None),
        ],
        recent: vec![
            recent("monzo-october.csv", "Current account", "Monzo", date(11, 2)),
            recent(
                "monzo-joint-october.csv",
                "Joint account",
                "Monzo",
                date(11, 2),
            ),
            recent("savings-2026-10.ofx", "Savings", "OFX", date(10, 31)),
        ],
        today: today(),
    }
}

#[cfg(test)]
mod tests {
    use coffer_core::statement::RowProblem;

    use super::*;
    use crate::import::session::Session;

    #[test]
    fn the_sample_reads_as_paper_shows_it() {
        let setup = setup();
        let session = Session::open(file(), &setup.presets, &setup.accounts, None).unwrap();
        assert_eq!(session.preset_id(), Some("uk-monzo"));
        // Two accounts use Monzo's preset, so the panel's selection decides.
        assert_eq!(session.account_id(), None);
        let statement = session.statement(&setup.presets, Currency::GBP).unwrap();
        assert_eq!(statement.rows().len(), 14);
        let flagged: Vec<_> = statement
            .rows()
            .iter()
            .enumerate()
            .filter(|(_, row)| row.outcome.is_err())
            .collect();
        assert_eq!(flagged.len(), 1);
        assert_eq!(flagged[0].0 + 1, 8);
        assert!(matches!(
            flagged[0].1.outcome,
            Err(RowProblem::Amount { .. })
        ));
        let pots = statement
            .rows()
            .iter()
            .filter_map(|row| row.outcome.as_ref().ok())
            .filter(|line| line.sub_account.is_some())
            .count();
        assert_eq!(pots, 2);
    }
}
