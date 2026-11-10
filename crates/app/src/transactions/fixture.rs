//! The Transactions screen's fixture data: the synthetic accounts and
//! transactions drawn on the Paper artboards Transactions (`18E-0`) and
//! Transactions · multi-select (`1NP-0`). Merchants, amounts and
//! descriptions are made up.
//!
//! Stands in for the database until M1. Amounts are integer pence, as the
//! real money type will be (`docs/ARCHITECTURE.md` → invariants); [`gbp`]
//! formats them until the money formatter exists (M1.5).

use gpui_kit::Rgba;

use crate::theme::tokens::color;
use crate::ui::marker::Marker;

/// A spending category, with its palette colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Groceries,
    EatingOut,
    Transport,
    Shopping,
    Bills,
    Entertainment,
    Other,
}

impl Category {
    /// Every category, in the left panel's order.
    pub const ALL: [Self; 7] = [
        Self::Groceries,
        Self::EatingOut,
        Self::Transport,
        Self::Shopping,
        Self::Bills,
        Self::Entertainment,
        Self::Other,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Groceries => "Groceries",
            Self::EatingOut => "Eating out",
            Self::Transport => "Transport",
            Self::Shopping => "Shopping",
            Self::Bills => "Bills",
            Self::Entertainment => "Entertainment",
            Self::Other => "Other",
        }
    }

    pub fn marker(self) -> Marker {
        let chart = |colour: Rgba| Marker::Colour(colour);
        match self {
            Self::Groceries => chart(color::CHART_1),
            Self::EatingOut => chart(color::CHART_2),
            Self::Shopping => chart(color::CHART_3),
            Self::Transport => chart(color::CHART_4),
            Self::Bills => chart(color::CHART_5),
            Self::Entertainment => chart(color::CHART_6),
            Self::Other => Marker::Other,
        }
    }

    /// How many transactions the category holds this month (the left
    /// panel's counts; fixed in the fixture).
    pub fn count(self) -> u32 {
        match self {
            Self::Groceries => 42,
            Self::EatingOut => 31,
            Self::Transport => 27,
            Self::Shopping => 18,
            Self::Bills => 9,
            Self::Entertainment => 6,
            Self::Other => 11,
        }
    }
}

/// Uncategorised transactions this month: the left panel's and the filter
/// chip's count.
pub const UNCATEGORISED_COUNT: u32 = 14;

/// A transaction's category, as far as it's decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Categorisation {
    Assigned(Category),
    Uncategorised,
    /// The agent proposed `category`; the user hasn't approved it.
    Suggested {
        category: Category,
        reason: &'static str,
    },
    /// A move between the user's own accounts or pots; left out of totals.
    Transfer {
        pot: &'static str,
    },
}

/// One day of the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Day {
    /// The day header: "Mon 9 Nov".
    pub heading: &'static str,
    /// The inspector's date field: "9 Nov 2026".
    pub date: &'static str,
    /// The multi-selection list: "9 Nov".
    pub short: &'static str,
}

/// One transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transaction {
    /// Stable identity, for selection and element ids.
    pub id: &'static str,
    /// Index into [`DAYS`].
    pub day: usize,
    pub merchant: &'static str,
    pub categorisation: Categorisation,
    pub account: &'static str,
    /// Signed pence: negative is money out.
    pub amount: i64,
    /// The amount in the original currency, already formatted.
    pub original_amount: Option<&'static str>,
    /// Failed reconciliation.
    pub flagged: bool,
    /// How the money moved: "Card payment", "Direct debit", …
    pub method: &'static str,
    /// Time of day, 24-hour.
    pub time: &'static str,
    /// The bank's description, as exported.
    pub raw_description: &'static str,
}

impl Transaction {
    pub fn is_transfer(&self) -> bool {
        matches!(self.categorisation, Categorisation::Transfer { .. })
    }
}

/// The days, newest first. Paper draws the first five; the sixth is below
/// the fold, so the list has somewhere to scroll.
pub const DAYS: [Day; 6] = [
    Day {
        heading: "Mon 9 Nov",
        date: "9 Nov 2026",
        short: "9 Nov",
    },
    Day {
        heading: "Sun 8 Nov",
        date: "8 Nov 2026",
        short: "8 Nov",
    },
    Day {
        heading: "Sat 7 Nov",
        date: "7 Nov 2026",
        short: "7 Nov",
    },
    Day {
        heading: "Fri 6 Nov",
        date: "6 Nov 2026",
        short: "6 Nov",
    },
    Day {
        heading: "Thu 5 Nov",
        date: "5 Nov 2026",
        short: "5 Nov",
    },
    Day {
        heading: "Wed 4 Nov",
        date: "4 Nov 2026",
        short: "4 Nov",
    },
];

const CURRENT: &str = "Current account";
const JOINT: &str = "Joint account";
const CARD: &str = "Card payment";

/// A card payment on `day` with no flags; [`Transaction::at`] sets its
/// time and the bank's text.
const fn card(
    id: &'static str,
    day: usize,
    merchant: &'static str,
    categorisation: Categorisation,
    account: &'static str,
    amount: i64,
) -> Transaction {
    Transaction {
        id,
        day,
        merchant,
        categorisation,
        account,
        amount,
        original_amount: None,
        flagged: false,
        method: CARD,
        time: "",
        raw_description: "",
    }
}

impl Transaction {
    /// The transaction at `time`, described by the bank as
    /// `raw_description`.
    const fn at(self, time: &'static str, raw_description: &'static str) -> Self {
        Self {
            time,
            raw_description,
            ..self
        }
    }
}

use Categorisation::{Assigned, Uncategorised};
use Category::{Bills, EatingOut, Entertainment, Groceries, Shopping, Transport};

/// The transactions, newest first; grouped by [`Transaction::day`].
pub const TRANSACTIONS: [Transaction; 25] = [
    card(
        "txn-pret-09",
        0,
        "Pret A Manger",
        Assigned(EatingOut),
        CURRENT,
        -645,
    )
    .at("08:12", "PRET A MANGER LONDON GB"),
    card(
        "txn-bluebottle-09",
        0,
        "SQ *BLUE BOTTLE COFFEE LDN",
        Uncategorised,
        CURRENT,
        -420,
    )
    .at("09:40", "SQ *BLUE BOTTLE COFFEE LDN GB"),
    Transaction {
        method: "Pot transfer",
        ..card(
            "txn-rainy-day-09",
            0,
            "To Rainy day",
            Categorisation::Transfer {
                pot: "Rainy day pot",
            },
            CURRENT,
            -25000,
        )
        .at("10:00", "TO POT RAINY DAY")
    },
    card(
        "txn-amazon-09",
        0,
        "Amazon Marketplace",
        Categorisation::Suggested {
            category: Shopping,
            reason: "23 earlier Amazon Marketplace payments are in Shopping.",
        },
        JOINT,
        -2399,
    )
    .at("14:32", "AMZN MKTP UK*2K4L09PZ5 AMAZON.CO.UK LU"),
    card(
        "txn-tfl-09",
        0,
        "Transport for London",
        Assigned(Transport),
        CURRENT,
        -310,
    )
    .at("18:05", "TFL TRAVEL CH TFL.GOV.UK/CP GB"),
    card(
        "txn-sainsburys-08",
        1,
        "Sainsbury's Local, Clapham High Street",
        Assigned(Groceries),
        CURRENT,
        -1862,
    )
    .at("11:20", "SAINSBURYS S/MKT CLAPHAM HIGH ST GB"),
    Transaction {
        original_amount: Some("€48.00"),
        ..card(
            "txn-comptoir-08",
            1,
            "Le Comptoir du Relais Saint-Germain, 9 Carrefour de l'Odéon, 75006 Paris",
            Assigned(EatingOut),
            CURRENT,
            -4187,
        )
        .at("13:15", "LE COMPTOIR DU RELAIS PARIS FR EUR 48.00")
    },
    Transaction {
        method: "Refund",
        ..card(
            "txn-asos-08",
            1,
            "ASOS.com",
            Assigned(Shopping),
            JOINT,
            3400,
        )
        .at("15:02", "ASOS.COM LTD REFUND GB")
    },
    card(
        "txn-deliveroo-08",
        1,
        "Deliveroo",
        Categorisation::Suggested {
            category: EatingOut,
            reason: "Every earlier Deliveroo payment is in Eating out.",
        },
        JOINT,
        -2745,
    )
    .at("19:48", "DELIVEROO.CO.UK LONDON GB"),
    card(
        "txn-uber-08",
        1,
        "Uber",
        Assigned(Transport),
        CURRENT,
        -1430,
    )
    .at("23:10", "UBER *TRIP HELP.UBER.COM GB"),
    Transaction {
        method: "Pot transfer",
        ..card(
            "txn-holiday-07",
            2,
            "From Holiday",
            Categorisation::Transfer { pot: "Holiday pot" },
            CURRENT,
            12000,
        )
        .at("09:00", "FROM POT HOLIDAY")
    },
    Transaction {
        flagged: true,
        method: "Direct debit",
        ..card(
            "txn-octopus-07",
            2,
            "Octopus Energy",
            Assigned(Bills),
            JOINT,
            -8640,
        )
        .at("06:00", "OCTOPUS ENERGY DD")
    },
    card(
        "txn-steam-07",
        2,
        "PAYPAL *STEAMGAMES 4029357733",
        Uncategorised,
        CURRENT,
        -899,
    )
    .at("21:34", "PAYPAL *STEAMGAMES 4029357733 GB"),
    card(
        "txn-odeon-07",
        2,
        "Odeon Luxe Leicester Square",
        Assigned(Entertainment),
        CURRENT,
        -3180,
    )
    .at("18:50", "ODEON LUXE LEICESTER SQ GB"),
    card(
        "txn-waitrose-07",
        2,
        "Waitrose & Partners, Clapham Junction",
        Assigned(Groceries),
        JOINT,
        -5216,
    )
    .at("12:31", "WAITROSE CLAPHAM JUNCTION GB"),
    card(
        "txn-netflix-07",
        2,
        "Netflix.com",
        Assigned(Entertainment),
        JOINT,
        -1299,
    )
    .at("04:00", "NETFLIX.COM AMSTERDAM NL"),
    card(
        "txn-gails-06",
        3,
        "Gail's Bakery",
        Categorisation::Suggested {
            category: EatingOut,
            reason: "Bakeries and cafés you've categorised are in Eating out.",
        },
        CURRENT,
        -560,
    )
    .at("08:45", "GAILS BAKERY CLAPHAM GB"),
    card(
        "txn-vinted-06",
        3,
        "VINTED *KX8F2Q",
        Uncategorised,
        CURRENT,
        -1675,
    )
    .at("20:12", "VINTED *KX8F2Q VINTED.CO.UK LT"),
    Transaction {
        method: "Direct debit",
        ..card(
            "txn-thames-06",
            3,
            "Thames Water",
            Assigned(Bills),
            JOINT,
            -3800,
        )
        .at("06:00", "THAMES WATER DD")
    },
    card(
        "txn-tesco-06",
        3,
        "Tesco Express",
        Assigned(Groceries),
        CURRENT,
        -985,
    )
    .at("19:02", "TESCO EXPRESS 3291 LONDON GB"),
    card(
        "txn-pret-05",
        4,
        "Pret A Manger",
        Assigned(EatingOut),
        CURRENT,
        -715,
    )
    .at("08:20", "PRET A MANGER LONDON GB"),
    card(
        "txn-trainline-05",
        4,
        "Trainline",
        Assigned(Transport),
        CURRENT,
        -4560,
    )
    .at("11:05", "TRAINLINE.COM LONDON GB"),
    card("txn-boots-05", 4, "Boots", Assigned(Shopping), JOINT, -1843)
        .at("17:40", "BOOTS 1189 CLAPHAM GB"),
    card(
        "txn-pret-04",
        5,
        "Pret A Manger",
        Assigned(EatingOut),
        CURRENT,
        -515,
    )
    .at("12:30", "PRET A MANGER LONDON GB"),
    card(
        "txn-tfl-04",
        5,
        "Transport for London",
        Assigned(Transport),
        CURRENT,
        -280,
    )
    .at("18:10", "TFL TRAVEL CH TFL.GOV.UK/CP GB"),
];

/// A row of the left panel's account list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Account {
    pub id: &'static str,
    pub name: &'static str,
    /// Pence.
    pub balance: i64,
    /// A pot, shown under its parent account.
    pub pot: bool,
}

/// The accounts, pots after their parent.
pub const ACCOUNTS: [Account; 6] = [
    Account {
        id: "account-current",
        name: CURRENT,
        balance: 128_410,
        pot: false,
    },
    Account {
        id: "pot-holiday",
        name: "Holiday",
        balance: 120_000,
        pot: true,
    },
    Account {
        id: "pot-rainy-day",
        name: "Rainy day",
        balance: 300_000,
        pot: true,
    },
    Account {
        id: "account-joint",
        name: JOINT,
        balance: -2_410,
        pot: false,
    },
    Account {
        id: "account-savings",
        name: "Savings",
        balance: 575_000,
        pot: false,
    },
    Account {
        id: "account-isa",
        name: "Stocks ISA",
        balance: 703_055,
        pot: false,
    },
];

/// The net worth figure: every balance, pots included (a pot's money isn't
/// in its parent's balance).
pub fn net_worth() -> i64 {
    ACCOUNTS.iter().map(|account| account.balance).sum()
}

/// How a formatted amount shows its sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    /// `−` for money out, `+` for money in: single transactions, and totals
    /// that can go either way.
    Always,
    /// `−` only below zero: balances, and totals whose label gives the
    /// direction.
    Negative,
}

/// `pence` as pounds for en-GB: "£1,284.10", with a true minus (U+2212)
/// and an explicit plus where `sign` asks (`docs/DESIGN.md` → "Money").
pub fn gbp(pence: i64, sign: Sign) -> String {
    let magnitude = pence.unsigned_abs();
    let pounds = (magnitude / 100).to_string();
    let mut grouped = String::new();
    for (index, digit) in pounds.chars().enumerate() {
        if index > 0 && (pounds.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    let prefix = match (pence < 0, sign) {
        (true, _) => "\u{2212}",
        (false, Sign::Always) if pence > 0 => "+",
        _ => "",
    };
    format!("{prefix}£{grouped}.{:02}", magnitude % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_pounds_with_a_true_minus_and_separators() {
        assert_eq!(gbp(-645, Sign::Always), "\u{2212}£6.45");
        assert_eq!(gbp(3400, Sign::Always), "+£34.00");
        assert_eq!(gbp(128_410, Sign::Negative), "£1,284.10");
        assert_eq!(gbp(-2_410, Sign::Negative), "\u{2212}£24.10");
        assert_eq!(gbp(1_824_055, Sign::Negative), "£18,240.55");
        assert_eq!(gbp(123_456_789, Sign::Negative), "£1,234,567.89");
        assert_eq!(gbp(0, Sign::Always), "£0.00");
    }

    #[test]
    fn net_worth_matches_the_panel() {
        assert_eq!(gbp(net_worth(), Sign::Negative), "£18,240.55");
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = TRANSACTIONS.iter().map(|txn| txn.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), TRANSACTIONS.len());
    }

    #[test]
    fn transactions_are_grouped_newest_first() {
        assert!(
            TRANSACTIONS
                .windows(2)
                .all(|pair| pair[0].day <= pair[1].day)
        );
        assert!(TRANSACTIONS.iter().all(|txn| txn.day < DAYS.len()));
    }
}
