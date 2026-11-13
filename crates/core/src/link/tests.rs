//! Tests for links and shares: netting in `ledger_line`, the personal view,
//! owed to you, suggestions and clearing.

use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::account::{self, AccountKind, NewAccount};
use crate::exclusion::{self, ExclusionReason};
use crate::manual;
use crate::split::{self, SplitLine};

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn day(month: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, d).unwrap()
}

fn label(text: &str) -> ShareLabel {
    ShareLabel::new(text).unwrap()
}

struct Fixture {
    db: Database,
    account: AccountId,
    _dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let mut db = Database::open(&dir.path().join("coffer.db")).unwrap();
        let account = account::create(
            &mut db,
            &NewAccount::new("Joint", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        Fixture {
            db,
            account,
            _dir: dir,
        }
    }

    fn conn(&self) -> &Connection {
        self.db.conn()
    }

    fn account(&mut self, name: &str, currency: Currency) -> AccountId {
        account::create(
            &mut self.db,
            &NewAccount::new(name, AccountKind::Current, currency),
        )
        .unwrap()
    }

    /// A manual row of `minor` GBP on `date` at `merchant`, in `category`.
    fn row_on(
        &self,
        account: AccountId,
        date: NaiveDate,
        minor: i64,
        merchant: &str,
        category: Option<&str>,
    ) -> TransactionId {
        let currency: String = self
            .conn()
            .query_row(
                "SELECT currency FROM account WHERE id = ?1",
                [account],
                |row| row.get(0),
            )
            .unwrap();
        self.conn()
            .execute(
                "INSERT INTO \"transaction\" (account_id, date, amount_minor, currency, \
                 merchant, merchant_display, description_raw, description_normalized, origin, \
                 category_id, category_source) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?5, upper(?5), 'manual', \
                 (SELECT id FROM category WHERE key = ?6), \
                 CASE WHEN ?6 IS NULL THEN NULL ELSE 'user' END)",
                params![account, date_key(date), minor, currency, merchant, category],
            )
            .unwrap();
        TransactionId::new(self.conn().last_insert_rowid())
    }

    fn row(
        &self,
        date: NaiveDate,
        minor: i64,
        merchant: &str,
        category: Option<&str>,
    ) -> TransactionId {
        self.row_on(self.account, date, minor, merchant, category)
    }

    fn link(
        &mut self,
        kind: LinkKind,
        inflow: TransactionId,
        target: TransactionId,
        minor: i64,
    ) -> LinkId {
        create(
            &mut self.db,
            &NewLink {
                kind,
                inflow,
                target: LinkTarget::Purchase(target),
                amount: gbp(minor),
            },
        )
        .unwrap()
    }

    fn refund(&mut self, inflow: TransactionId, target: TransactionId, minor: i64) -> LinkId {
        self.link(LinkKind::Refund, inflow, target, minor)
    }

    /// The personal spending (or income) in `category` in `month`
    /// (`YYYY-MM`), as reports read it.
    fn personal(&self, category: &str, month: &str) -> i64 {
        self.conn()
            .query_row(
                "SELECT coalesce(sum(l.personal_minor), 0) FROM ledger_line l \
                 JOIN category c ON c.id = l.category_id \
                 WHERE (c.key = ?1 OR c.key LIKE ?1 || '.%') \
                   AND substr(l.date, 1, 7) = ?2",
                params![category, month],
                |row| row.get(0),
            )
            .unwrap()
    }

    /// The gross amount in `category` in `month`, of rows in reports.
    fn gross(&self, category: &str, month: &str) -> i64 {
        self.conn()
            .query_row(
                "SELECT coalesce(sum(l.amount_minor), 0) FROM ledger_line l \
                 JOIN category c ON c.id = l.category_id \
                 WHERE (c.key = ?1 OR c.key LIKE ?1 || '.%') \
                   AND substr(l.date, 1, 7) = ?2 AND l.in_reports",
                params![category, month],
                |row| row.get(0),
            )
            .unwrap()
    }

    /// `transaction`'s ledger rows: `(amount, category key, my amount,
    /// personal, link kind, in reports)`, own row first.
    #[allow(clippy::type_complexity)]
    fn ledger(
        &self,
        transaction: TransactionId,
    ) -> Vec<(i64, Option<String>, i64, i64, Option<String>, bool)> {
        let mut statement = self
            .conn()
            .prepare(
                "SELECT l.amount_minor, c.key, l.my_amount_minor, l.personal_minor, \
                 l.link_kind, l.in_reports FROM ledger_line l \
                 LEFT JOIN category c ON c.id = l.category_id \
                 WHERE l.transaction_id = ?1 \
                 ORDER BY l.link_id IS NOT NULL, l.link_id, l.target_split_id, l.line_no",
            )
            .unwrap();
        statement
            .query_map([transaction], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// Every transaction's ledger rows sum to its amount.
    #[track_caller]
    fn assert_ledger_sums(&self) {
        let mismatched: i64 = self
            .conn()
            .query_row(
                "SELECT count(*) FROM \"transaction\" t WHERE t.amount_minor IS NOT \
                 (SELECT sum(amount_minor) FROM ledger_line l WHERE l.transaction_id = t.id)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(mismatched, 0);
    }

    fn statuses(&self) -> Vec<(String, i64, Option<i64>, i64, String)> {
        let mut statement = self
            .conn()
            .prepare(
                "SELECT kind, inflow_id, target_id, amount_minor, status FROM transaction_link \
                 ORDER BY id",
            )
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }
}

fn s(text: &str) -> Option<String> {
    Some(text.to_owned())
}

// Keys ------------------------------------------------------------------------

#[test]
fn keys_match_the_schema() {
    let fixture = Fixture::new();
    let sql: String = fixture
        .conn()
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name = 'transaction_link'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for kind in LinkKind::ALL {
        assert!(sql.contains(&format!("'{}'", kind.as_str())));
        assert_eq!(LinkKind::from_key(kind.as_str()), Some(kind));
    }
    for status in LinkStatus::ALL {
        assert!(sql.contains(&format!("'{}'", status.as_str())));
        assert_eq!(LinkStatus::from_key(status.as_str()), Some(status));
    }
}

#[test]
fn share_labels_are_trimmed_and_checked() {
    assert_eq!(label("  Sam ").as_str(), "Sam");
    assert!(matches!(ShareLabel::new(" \t"), Err(Error::BlankLabel)));
    assert!(matches!(
        ShareLabel::new("Sam\nX"),
        Err(Error::InvalidLabel)
    ));
    assert!(matches!(
        ShareLabel::new(&"x".repeat(65)),
        Err(Error::InvalidLabel)
    ));
    assert!(ShareLabel::new(&"é".repeat(64)).is_ok());
    // A label never shows in debug output.
    assert!(!format!("{:?}", label("Sam Secret")).contains("Sam"));
}

// Refunds in the ledger -------------------------------------------------------

#[test]
fn a_full_refund_nets_against_the_purchase_in_its_own_month() {
    let mut fixture = Fixture::new();
    let purchase = fixture.row(day(10, 28), -6800, "ASOS.com", Some("shopping"));
    let refund = fixture.row(day(11, 8), 6800, "ASOS.com", Some("income"));
    assert_eq!(fixture.personal("income", "2026-11"), 6800);

    fixture.refund(refund, purchase, 6800);

    // The refund's own row keeps nothing; its linked part is Shopping.
    assert_eq!(
        fixture.ledger(refund),
        [
            (0, s("income"), 0, 0, None, true),
            (6800, s("shopping"), 6800, 6800, s("refund"), true),
        ]
    );
    // Never income, and the purchase's month stays as it was.
    assert_eq!(fixture.personal("income", "2026-11"), 0);
    assert_eq!(fixture.personal("shopping", "2026-10"), -6800);
    assert_eq!(fixture.personal("shopping", "2026-11"), 6800);
    assert_eq!(purchase_left(&fixture.db, purchase).unwrap(), gbp(0));
    assert_eq!(inflow_left(&fixture.db, refund).unwrap(), gbp(0));
    fixture.assert_ledger_sums();
}

#[test]
fn a_partial_refund_leaves_the_rest_to_refund() {
    let mut fixture = Fixture::new();
    let purchase = fixture.row(day(10, 28), -6800, "ASOS.com", Some("shopping"));
    let first = fixture.row(day(11, 8), 3400, "ASOS.com", None);
    fixture.refund(first, purchase, 3400);
    assert_eq!(purchase_left(&fixture.db, purchase).unwrap(), gbp(3400));
    assert_eq!(fixture.personal("shopping", "2026-11"), 3400);

    // A second refund can take the rest, and no more.
    let second = fixture.row(day(11, 9), 5000, "ASOS.com", None);
    match create(
        &mut fixture.db,
        &NewLink {
            kind: LinkKind::Refund,
            inflow: second,
            target: LinkTarget::Purchase(purchase),
            amount: gbp(5000),
        },
    ) {
        Err(Error::ExceedsPurchase { left }) => assert_eq!(left, gbp(3400)),
        other => panic!("expected the purchase's cap, got {other:?}"),
    }
    fixture.refund(second, purchase, 3400);
    assert_eq!(purchase_left(&fixture.db, purchase).unwrap(), gbp(0));
    fixture.assert_ledger_sums();
}

#[test]
fn an_overpayment_is_capped_and_the_rest_counts_by_its_own_category() {
    let mut fixture = Fixture::new();
    let purchase = fixture.row(day(11, 1), -6800, "ASOS.com", Some("shopping"));
    let refund = fixture.row(day(11, 8), 8000, "ASOS.com", Some("income"));
    fixture.refund(refund, purchase, 6800);

    assert_eq!(inflow_left(&fixture.db, refund).unwrap(), gbp(1200));
    assert_eq!(
        fixture.ledger(refund),
        [
            (1200, s("income"), 1200, 1200, None, true),
            (6800, s("shopping"), 6800, 6800, s("refund"), true),
        ]
    );
    assert_eq!(fixture.personal("shopping", "2026-11"), 0);
    assert_eq!(fixture.personal("income", "2026-11"), 1200);
    // The money in can't give more than it has.
    let other = fixture.row(day(11, 2), -5000, "ASOS.com", Some("shopping"));
    match create(
        &mut fixture.db,
        &NewLink {
            kind: LinkKind::Refund,
            inflow: refund,
            target: LinkTarget::Purchase(other),
            amount: gbp(1300),
        },
    ) {
        Err(Error::ExceedsInflow { left }) => assert_eq!(left, gbp(1200)),
        other => panic!("expected the inflow's cap, got {other:?}"),
    }
    fixture.assert_ledger_sums();
}

#[test]
fn a_refund_of_a_split_purchase_is_divided_by_largest_remainder() {
    let mut fixture = Fixture::new();
    let key = |key: &str| Some(crate::category::CategoryKey::new(key).unwrap());
    let purchase = fixture.row(day(11, 1), -1000, "Shop", None);
    split::set(
        &mut fixture.db,
        purchase,
        &[
            SplitLine::new(gbp(-333), key("food")),
            SplitLine::new(gbp(-667), key("home")),
        ],
    )
    .unwrap();
    let refund = fixture.row(day(11, 2), 500, "Shop", None);
    fixture.refund(refund, purchase, 500);

    // Exact parts 166.5 and 333.5: floors 166 and 333, and the unit left
    // goes to the tie's lower line.
    assert_eq!(
        fixture.ledger(refund),
        [
            (0, None, 0, 0, None, true),
            (167, s("food"), 167, 167, s("refund"), true),
            (333, s("home"), 333, 333, s("refund"), true),
        ]
    );

    // Three lines of 1 and a refund of 2: no line gets more than it had.
    let small = fixture.row(day(11, 1), -3, "Shop", None);
    split::set(
        &mut fixture.db,
        small,
        &[
            SplitLine::new(gbp(-1), key("food")),
            SplitLine::new(gbp(-1), key("home")),
            SplitLine::new(gbp(-1), key("shopping")),
        ],
    )
    .unwrap();
    let back = fixture.row(day(11, 2), 2, "Shop", None);
    fixture.refund(back, small, 2);
    assert_eq!(
        fixture.ledger(back),
        [
            (0, None, 0, 0, None, true),
            (1, s("food"), 1, 1, s("refund"), true),
            (1, s("home"), 1, 1, s("refund"), true),
        ]
    );
    fixture.assert_ledger_sums();

    // Splitting the purchase again moves the refund with it.
    split::set(
        &mut fixture.db,
        purchase,
        &[
            SplitLine::new(gbp(-500), key("food")),
            SplitLine::new(gbp(-500), key("home")),
        ],
    )
    .unwrap();
    assert_eq!(fixture.ledger(refund)[1].0, 250);
    fixture.assert_ledger_sums();
}

#[test]
fn a_linked_inflow_isnt_split() {
    let mut fixture = Fixture::new();
    let purchase = fixture.row(day(11, 1), -1000, "Shop", Some("shopping"));
    let refund = fixture.row(day(11, 2), 1000, "Shop", None);
    fixture.refund(refund, purchase, 600);
    let lines = [
        SplitLine::new(gbp(500), None),
        SplitLine::new(gbp(500), None),
    ];
    assert!(matches!(
        split::set(&mut fixture.db, refund, &lines),
        Err(split::Error::Linked(id)) if id == refund
    ));

    // A suggestion doesn't stand in the way: splitting drops it.
    fixture.row(day(11, 1), -1000, "Other", None);
    let other = fixture.row(day(11, 3), 1000, "Other", None);
    // The refund's pair is linked already, so only the other is suggested.
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 1);
    split::set(&mut fixture.db, other, &lines).unwrap();
    assert!(fixture.statuses().iter().all(|link| link.1 != other.get()));
}

#[test]
fn a_refund_of_an_excluded_charge_leaves_reports_with_it() {
    let mut fixture = Fixture::new();
    let charge = fixture.row(day(11, 1), -2500, "Shop", Some("shopping"));
    exclusion::exclude(&mut fixture.db, charge, ExclusionReason::BankError, None).unwrap();
    let refund = fixture.row(day(11, 5), 2500, "Shop", Some("income"));
    fixture.refund(refund, charge, 2500);

    assert_eq!(fixture.personal("shopping", "2026-11"), 0);
    assert_eq!(fixture.gross("shopping", "2026-11"), 0);
    assert_eq!(fixture.personal("income", "2026-11"), 0);
    // Both stay in the balance: the charge happened and so did the refund.
    let balance: i64 = fixture
        .conn()
        .query_row(
            "SELECT sum(amount_minor) FROM ledger_line \
             WHERE excluded_reason IS NOT 'duplicate'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(balance, 0);
}

// Shares and claims -----------------------------------------------------------

#[test]
fn spending_is_my_share_and_owed_is_the_rest() {
    let mut fixture = Fixture::new();
    let odeon = fixture.row(day(11, 7), -3180, "Odeon", Some("entertainment"));
    set_sharing(
        &mut fixture.db,
        odeon,
        &Sharing::Shared {
            my_share: gbp(-1590),
            label: label("Sam"),
        },
    )
    .unwrap();
    assert_eq!(fixture.personal("entertainment", "2026-11"), -1590);
    // Gross stays the bank's.
    assert_eq!(fixture.gross("entertainment", "2026-11"), -3180);

    // A split line shares on its own.
    let dinner = fixture.row(day(11, 9), -6000, "Bistro", None);
    let key = |key: &str| Some(crate::category::CategoryKey::new(key).unwrap());
    let ids = split::set(
        &mut fixture.db,
        dinner,
        &[
            SplitLine::new(gbp(-4000), key("food")),
            SplitLine::new(gbp(-2000), key("food")),
        ],
    )
    .unwrap();
    set_line_share(
        &mut fixture.db,
        ids[0],
        Some(&LineShare {
            my_share: gbp(-1000),
            label: label("Sam"),
        }),
    )
    .unwrap();
    set_line_share(
        &mut fixture.db,
        ids[1],
        Some(&LineShare {
            my_share: gbp(-500),
            label: label("Alex"),
        }),
    )
    .unwrap();
    assert_eq!(fixture.personal("food", "2026-11"), -1500);

    let owed = owed_by_label(&fixture.db).unwrap();
    assert_eq!(
        owed,
        [
            PersonOwed {
                label: label("Alex"),
                owed: gbp(1500),
                costs: 1,
                first_cost: Some(day(11, 9)),
                last_paid: None,
            },
            PersonOwed {
                label: label("Sam"),
                owed: gbp(1590 + 3000),
                costs: 2,
                first_cost: Some(day(11, 7)),
                last_paid: None,
            },
        ]
    );
    assert_eq!(
        share_labels(&fixture.db).unwrap(),
        [label("Alex"), label("Sam")]
    );

    // Just me counts it all again.
    set_sharing(&mut fixture.db, odeon, &Sharing::JustMe).unwrap();
    assert_eq!(fixture.personal("entertainment", "2026-11"), -3180);
}

#[test]
fn sharing_is_checked() {
    let mut fixture = Fixture::new();
    let purchase = fixture.row(day(11, 7), -3180, "Odeon", None);
    let shared = |minor: i64| Sharing::Shared {
        my_share: gbp(minor),
        label: label("Sam"),
    };
    assert!(matches!(
        set_sharing(&mut fixture.db, purchase, &shared(-3181)),
        Err(Error::ShareOutsideAmount)
    ));
    assert!(matches!(
        set_sharing(&mut fixture.db, purchase, &shared(10)),
        Err(Error::ShareOutsideAmount)
    ));
    assert!(matches!(
        set_sharing(
            &mut fixture.db,
            purchase,
            &Sharing::Shared {
                my_share: Money::new(-100, Currency::EUR),
                label: label("Sam"),
            }
        ),
        Err(Error::ShareCurrency { .. })
    ));
    let income = fixture.row(day(11, 7), 3180, "Employer", None);
    assert!(matches!(
        set_sharing(&mut fixture.db, income, &Sharing::ClaimBack),
        Err(Error::NotMoneyOut(_))
    ));

    // Shared and claimed back exclude each other.
    set_sharing(&mut fixture.db, purchase, &shared(-1000)).unwrap();
    set_sharing(&mut fixture.db, purchase, &Sharing::ClaimBack).unwrap();
    let fields: (Option<i64>, Option<String>, bool) = fixture
        .conn()
        .query_row(
            "SELECT my_share_minor, share_label, reimbursable FROM \"transaction\" WHERE id = ?1",
            [purchase],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(fields, (None, None, true));

    // A split purchase shares per line, and not while claimed as a whole.
    let key = |key: &str| Some(crate::category::CategoryKey::new(key).unwrap());
    let split_row = fixture.row(day(11, 8), -1000, "Shop", None);
    let ids = split::set(
        &mut fixture.db,
        split_row,
        &[
            SplitLine::new(gbp(-600), key("food")),
            SplitLine::new(gbp(-400), key("home")),
        ],
    )
    .unwrap();
    assert!(matches!(
        set_sharing(&mut fixture.db, split_row, &shared(-500)),
        Err(Error::Split(_))
    ));
    set_sharing(&mut fixture.db, split_row, &Sharing::ClaimBack).unwrap();
    assert!(matches!(
        set_line_share(
            &mut fixture.db,
            ids[0],
            Some(&LineShare {
                my_share: gbp(-300),
                label: label("Sam")
            })
        ),
        Err(Error::ClaimedBack(_))
    ));
}

#[test]
fn a_claim_counts_as_nothing_until_set_back_to_just_me() {
    let mut fixture = Fixture::new();
    let train = fixture.row(day(11, 2), -3860, "Trainline", Some("transport"));
    set_sharing(&mut fixture.db, train, &Sharing::ClaimBack).unwrap();
    assert_eq!(fixture.personal("transport", "2026-11"), 0);
    assert_eq!(fixture.gross("transport", "2026-11"), -3860);
    let claims = outstanding_claims(&fixture.db).unwrap();
    assert_eq!(
        claims,
        [Claim {
            transaction: train,
            account: fixture.account,
            date: day(11, 2),
            spent: gbp(-3860),
            owed: gbp(3860),
        }]
    );

    // A part payment leaves the rest owed, and counts as nothing either.
    let paid = fixture.row(day(11, 9), 2000, "ACME LTD EXPENSES", Some("income"));
    fixture.link(LinkKind::Reimbursement, paid, train, 2000);
    assert_eq!(outstanding_claims(&fixture.db).unwrap()[0].owed, gbp(1860));
    assert_eq!(fixture.personal("transport", "2026-11"), 0);
    assert_eq!(fixture.personal("income", "2026-11"), 0);
    assert_eq!(owed_total(&fixture.db).unwrap(), [gbp(1860)]);

    // A claim that won't be paid counts in full, less what was repaid.
    set_sharing(&mut fixture.db, train, &Sharing::JustMe).unwrap();
    assert_eq!(fixture.personal("transport", "2026-11"), -1860);
    assert!(outstanding_claims(&fixture.db).unwrap().is_empty());

    // Repaid in full, it counts as nothing and leaves the list.
    let taxi = fixture.row(day(11, 3), -1500, "Uber", Some("transport"));
    set_sharing(&mut fixture.db, taxi, &Sharing::ClaimBack).unwrap();
    let rest = fixture.row(day(11, 10), 1500, "ACME LTD EXPENSES", None);
    fixture.link(LinkKind::Reimbursement, rest, taxi, 1500);
    assert_eq!(fixture.personal("transport", "2026-11"), -1860);
    assert!(outstanding_claims(&fixture.db).unwrap().is_empty());
    fixture.assert_ledger_sums();
}

#[test]
fn a_reimbursement_repays_a_claim() {
    let mut fixture = Fixture::new();
    let purchase = fixture.row(day(11, 2), -3860, "Trainline", None);
    let paid = fixture.row(day(11, 9), 3860, "ACME", None);
    let link = NewLink {
        kind: LinkKind::Reimbursement,
        inflow: paid,
        target: LinkTarget::Purchase(purchase),
        amount: gbp(3860),
    };
    assert!(matches!(
        create(&mut fixture.db, &link),
        Err(Error::NotClaim(id)) if id == purchase
    ));
    let settlement = NewLink {
        kind: LinkKind::Settlement,
        ..link.clone()
    };
    assert!(matches!(
        create(&mut fixture.db, &settlement),
        Err(Error::WrongTarget { .. })
    ));
}

#[test]
fn settlements_reduce_what_a_person_owes() {
    let mut fixture = Fixture::new();
    let odeon = fixture.row(day(11, 7), -3180, "Odeon", Some("entertainment"));
    set_sharing(
        &mut fixture.db,
        odeon,
        &Sharing::Shared {
            my_share: gbp(-1590),
            label: label("Sam"),
        },
    )
    .unwrap();
    let from_sam = fixture.row(day(11, 12), 2000, "S SMITH", Some("income"));
    let settle = |amount: i64| NewLink {
        kind: LinkKind::Settlement,
        inflow: from_sam,
        target: LinkTarget::Person(label("Sam")),
        amount: gbp(amount),
    };
    match create(&mut fixture.db, &settle(1591)) {
        Err(Error::ExceedsOwed { owed }) => assert_eq!(owed, gbp(1590)),
        other => panic!("expected what Sam owes, got {other:?}"),
    }
    create(&mut fixture.db, &settle(1000)).unwrap();

    let owed = owed_by_label(&fixture.db).unwrap();
    assert_eq!(owed[0].owed, gbp(590));
    assert_eq!(owed[0].last_paid, Some(day(11, 12)));
    assert_eq!(
        owed_to(&fixture.db, &label("Sam"), Currency::GBP).unwrap(),
        gbp(590)
    );
    // Sam's payment isn't the user's income; the rest of it is.
    assert_eq!(
        fixture.ledger(from_sam),
        [
            (1000, s("income"), 1000, 1000, None, true),
            (1000, None, 0, 0, s("settlement"), true),
        ]
    );
    fixture.assert_ledger_sums();
}

#[test]
fn a_refund_of_a_shared_purchase_refunds_both_parts() {
    let mut fixture = Fixture::new();
    let odeon = fixture.row(day(11, 7), -3180, "Odeon", Some("entertainment"));
    set_sharing(
        &mut fixture.db,
        odeon,
        &Sharing::Shared {
            my_share: gbp(-1590),
            label: label("Sam"),
        },
    )
    .unwrap();
    let refund = fixture.row(day(11, 8), 3180, "Odeon", None);
    fixture.refund(refund, odeon, 3180);
    assert_eq!(fixture.personal("entertainment", "2026-11"), 0);
    assert_eq!(
        owed_to(&fixture.db, &label("Sam"), Currency::GBP).unwrap(),
        gbp(0)
    );
}

#[test]
fn renaming_a_label_renames_it_everywhere() {
    let mut fixture = Fixture::new();
    let odeon = fixture.row(day(11, 7), -3180, "Odeon", None);
    set_sharing(
        &mut fixture.db,
        odeon,
        &Sharing::Shared {
            my_share: gbp(-1590),
            label: label("Sam"),
        },
    )
    .unwrap();
    let from_sam = fixture.row(day(11, 12), 1000, "S SMITH", None);
    create(
        &mut fixture.db,
        &NewLink {
            kind: LinkKind::Settlement,
            inflow: from_sam,
            target: LinkTarget::Person(label("Sam")),
            amount: gbp(1000),
        },
    )
    .unwrap();
    assert_eq!(
        rename_label(&mut fixture.db, &label("Sam"), &label("Sam S")).unwrap(),
        2
    );
    assert_eq!(share_labels(&fixture.db).unwrap(), [label("Sam S")]);
    assert_eq!(
        owed_to(&fixture.db, &label("Sam S"), Currency::GBP).unwrap(),
        gbp(590)
    );
}

// Links: checks ---------------------------------------------------------------

#[test]
fn links_need_money_in_money_out_and_one_currency() {
    let mut fixture = Fixture::new();
    let euros = fixture.account("Euro", Currency::EUR);
    let purchase = fixture.row(day(11, 1), -1000, "Shop", None);
    let other_purchase = fixture.row(day(11, 1), -1000, "Shop", None);
    let refund = fixture.row(day(11, 2), 1000, "Shop", None);
    let euro_refund = fixture.row_on(euros, day(11, 2), 1000, "Shop", None);
    let try_link = |fixture: &mut Fixture, inflow, target, amount: Money| {
        create(
            &mut fixture.db,
            &NewLink {
                kind: LinkKind::Refund,
                inflow,
                target: LinkTarget::Purchase(target),
                amount,
            },
        )
    };
    assert!(matches!(
        try_link(&mut fixture, purchase, other_purchase, gbp(100)),
        Err(Error::NotMoneyIn(_))
    ));
    assert!(matches!(
        try_link(&mut fixture, refund, euro_refund, gbp(100)),
        Err(Error::NotMoneyOut(_))
    ));
    assert!(matches!(
        try_link(
            &mut fixture,
            euro_refund,
            purchase,
            Money::new(100, Currency::EUR)
        ),
        Err(Error::CurrencyMismatch { .. })
    ));
    assert!(matches!(
        try_link(&mut fixture, refund, purchase, gbp(0)),
        Err(Error::NotPositive)
    ));
    assert!(matches!(
        try_link(
            &mut fixture,
            refund,
            purchase,
            Money::new(100, Currency::EUR)
        ),
        Err(Error::CurrencyMismatch { .. })
    ));
    let id = try_link(&mut fixture, refund, purchase, gbp(100)).unwrap();
    assert!(matches!(
        try_link(&mut fixture, refund, purchase, gbp(100)),
        Err(Error::AlreadyLinked(existing)) if existing == id
    ));
}

#[test]
fn a_transfer_isnt_linked() {
    let mut fixture = Fixture::new();
    let purchase = fixture.row(day(11, 1), -1000, "Shop", None);
    let refund = fixture.row(day(11, 2), 1000, "Shop", None);
    fixture
        .conn()
        .execute(
            "INSERT INTO transfer_group (status) VALUES ('suggested')",
            [],
        )
        .unwrap();
    let group = fixture.conn().last_insert_rowid();
    fixture
        .conn()
        .execute(
            "UPDATE \"transaction\" SET transfer_group_id = ?1 WHERE id = ?2",
            params![group, refund],
        )
        .unwrap();
    assert!(matches!(
        create(
            &mut fixture.db,
            &NewLink {
                kind: LinkKind::Refund,
                inflow: refund,
                target: LinkTarget::Purchase(purchase),
                amount: gbp(1000),
            }
        ),
        Err(Error::InTransfer(id)) if id == refund
    ));
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 0);
}

#[test]
fn rejecting_unlinks_and_confirm_links_again() {
    let mut fixture = Fixture::new();
    let purchase = fixture.row(day(11, 1), -1000, "Shop", Some("shopping"));
    let refund = fixture.row(day(11, 2), 1000, "Shop", None);
    let id = fixture.refund(refund, purchase, 1000);
    reject(&mut fixture.db, id).unwrap();
    assert_eq!(get(&fixture.db, id).unwrap().status, LinkStatus::Rejected);
    assert_eq!(purchase_left(&fixture.db, purchase).unwrap(), gbp(1000));
    assert_eq!(fixture.personal("shopping", "2026-11"), -1000);
    // Not suggested again.
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 0);
    // Undo.
    confirm(&mut fixture.db, id).unwrap();
    assert_eq!(fixture.personal("shopping", "2026-11"), 0);
    assert_eq!(links_of(&fixture.db, purchase).unwrap().len(), 1);

    delete(&mut fixture.db, id).unwrap();
    assert!(matches!(
        delete(&mut fixture.db, id),
        Err(Error::LinkNotFound(_))
    ));
    // A deleted pair can be suggested again.
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 1);
}

#[test]
fn confirming_drops_suggestions_that_no_longer_fit() {
    let mut fixture = Fixture::new();
    let purchase = fixture.row(day(11, 1), -1000, "Shop", Some("shopping"));
    let first = fixture.row(day(11, 2), 700, "Shop", None);
    let second = fixture.row(day(11, 3), 700, "Shop", None);
    // The second refund doesn't fit beside the first suggestion.
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 1);
    // The user links the second by hand; the first's suggestion no longer
    // fits and goes.
    fixture.refund(second, purchase, 700);
    assert_eq!(
        fixture.statuses(),
        [(
            "refund".to_owned(),
            second.get(),
            Some(purchase.get()),
            700,
            "confirmed".to_owned()
        )]
    );
    // And the first can't be confirmed over the cap.
    let _ = first;
}

// Suggestions -----------------------------------------------------------------

#[test]
fn a_refund_is_suggested_within_90_days_on_the_same_account_and_merchant() {
    let mut fixture = Fixture::new();
    let savings = fixture.account("Savings", Currency::GBP);
    let old = fixture.row(day(8, 9), -6800, "ASOS.com", None); // 91 days before
    let in_window = fixture.row(day(8, 10), -6800, "ASOS.com", None); // 90 days
    let _other_merchant = fixture.row(day(11, 1), -6800, "Zara", None);
    let _other_account = fixture.row_on(savings, day(11, 1), -6800, "ASOS.com", None);
    let _too_small = fixture.row(day(11, 2), -3000, "ASOS.com", None);
    let refund = fixture.row(day(11, 8), 3400, "asos.COM", None);

    assert_eq!(suggest_all(fixture.conn()).unwrap(), 1);
    assert_eq!(
        fixture.statuses(),
        [(
            "refund".to_owned(),
            refund.get(),
            Some(in_window.get()),
            3400,
            "suggested".to_owned()
        )]
    );
    let _ = old;
    // Running again suggests nothing new.
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 0);
}

#[test]
fn an_exact_purchase_goes_first_then_the_newest() {
    let fixture = Fixture::new();
    let exact = fixture.row(day(10, 1), -3400, "ASOS.com", None);
    let _newer = fixture.row(day(11, 1), -6800, "ASOS.com", None);
    let refund = fixture.row(day(11, 8), 3400, "ASOS.com", None);
    suggest_all(fixture.conn()).unwrap();
    let links = links_of(&fixture.db, refund).unwrap();
    assert_eq!(links[0].target, LinkTarget::Purchase(exact));

    // Without an exact one, the newest.
    let fixture = Fixture::new();
    let _older = fixture.row(day(10, 1), -6800, "ASOS.com", None);
    let newer = fixture.row(day(11, 1), -6800, "ASOS.com", None);
    let refund = fixture.row(day(11, 8), 3400, "ASOS.com", None);
    suggest_all(fixture.conn()).unwrap();
    let links = links_of(&fixture.db, refund).unwrap();
    assert_eq!(links[0].target, LinkTarget::Purchase(newer));
}

#[test]
fn a_rejected_pair_is_never_suggested_again() {
    let mut fixture = Fixture::new();
    let purchase = fixture.row(day(11, 1), -6800, "ASOS.com", None);
    let refund = fixture.row(day(11, 8), 3400, "ASOS.com", None);
    suggest_all(fixture.conn()).unwrap();
    let id = links_of(&fixture.db, refund).unwrap()[0].id;
    reject(&mut fixture.db, id).unwrap();
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 0);
    // The schema holds it too.
    assert!(
        fixture
            .conn()
            .execute(
                "UPDATE transaction_link SET status = 'suggested' WHERE id = ?1",
                [id]
            )
            .is_err()
    );
    // Another purchase can still be suggested for it.
    let other = fixture.row(day(11, 2), -3400, "ASOS.com", None);
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 1);
    assert_eq!(
        links_of(&fixture.db, refund).unwrap()[1].target,
        LinkTarget::Purchase(other)
    );
    let _ = purchase;
}

#[test]
fn a_duplicate_is_never_a_refunds_purchase_but_a_bank_error_is() {
    let mut fixture = Fixture::new();
    let duplicate = fixture.row(day(11, 2), -2500, "Shop", None);
    exclusion::exclude(&mut fixture.db, duplicate, ExclusionReason::Duplicate, None).unwrap();
    let refund = fixture.row(day(11, 5), 2500, "Shop", None);
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 0);

    let charge = fixture.row(day(11, 1), -2500, "Shop", None);
    exclusion::exclude(&mut fixture.db, charge, ExclusionReason::BankError, None).unwrap();
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 1);
    assert_eq!(
        links_of(&fixture.db, refund).unwrap()[0].target,
        LinkTarget::Purchase(charge)
    );
}

#[test]
fn a_reimbursement_is_suggested_for_one_claim_or_a_set() {
    let mut fixture = Fixture::new();
    let claim = |fixture: &mut Fixture, date, minor, merchant| {
        let id = fixture.row(date, minor, merchant, None);
        set_sharing(&mut fixture.db, id, &Sharing::ClaimBack).unwrap();
        id
    };
    let train = claim(&mut fixture, day(10, 30), -12000, "Trainline");
    let hotel = claim(&mut fixture, day(10, 31), -3060, "Premier Inn");
    let uber = claim(&mut fixture, day(11, 1), -1500, "Uber");
    let paid = fixture.row(day(11, 9), 15060, "ACME LTD EXPENSES", None);

    assert_eq!(suggest_all(fixture.conn()).unwrap(), 2);
    let mut targets: Vec<_> = links_of(&fixture.db, paid)
        .unwrap()
        .into_iter()
        .map(|link| (link.kind, link.target, link.amount, link.status))
        .collect();
    targets.sort_by_key(|link| format!("{:?}", link.1));
    assert_eq!(
        targets,
        [
            (
                LinkKind::Reimbursement,
                LinkTarget::Purchase(train),
                gbp(12000),
                LinkStatus::Suggested
            ),
            (
                LinkKind::Reimbursement,
                LinkTarget::Purchase(hotel),
                gbp(3060),
                LinkStatus::Suggested
            ),
        ]
    );

    // Confirming both clears both claims.
    for link in links_of(&fixture.db, paid).unwrap() {
        confirm(&mut fixture.db, link.id).unwrap();
    }
    let open: Vec<_> = outstanding_claims(&fixture.db)
        .unwrap()
        .into_iter()
        .map(|claim| claim.transaction)
        .collect();
    assert_eq!(open, [uber]);

    // One claim, exactly.
    let single = fixture.row(day(11, 10), 1500, "ACME LTD EXPENSES", None);
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 1);
    assert_eq!(
        links_of(&fixture.db, single).unwrap()[0].target,
        LinkTarget::Purchase(uber)
    );
}

#[test]
fn the_fewest_claims_win_then_the_newest() {
    let mut fixture = Fixture::new();
    let claim = |fixture: &mut Fixture, date, minor| {
        let id = fixture.row(date, minor, "Expense", None);
        set_sharing(&mut fixture.db, id, &Sharing::ClaimBack).unwrap();
        id
    };
    let _a = claim(&mut fixture, day(11, 1), -1000);
    let _b = claim(&mut fixture, day(11, 2), -2000);
    let older_whole = claim(&mut fixture, day(11, 3), -3000);
    let newer_whole = claim(&mut fixture, day(11, 4), -3000);
    let paid = fixture.row(day(11, 9), 3000, "ACME", None);
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 1);
    assert_eq!(
        links_of(&fixture.db, paid).unwrap()[0].target,
        LinkTarget::Purchase(newer_whole)
    );
    let _ = older_whole;
}

#[test]
fn a_reimbursement_waits_at_most_60_days_after_the_newest_claim() {
    let mut fixture = Fixture::new();
    let claim = fixture.row(day(9, 9), -5000, "Hotel", None); // 61 days before
    set_sharing(&mut fixture.db, claim, &Sharing::ClaimBack).unwrap();
    let _late = fixture.row(day(11, 9), 5000, "ACME", None);
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 0);

    let in_time = fixture.row(day(11, 8), 5000, "ACME", None); // 60 days
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 1);
    assert_eq!(
        links_of(&fixture.db, in_time).unwrap()[0].target,
        LinkTarget::Purchase(claim)
    );

    // An older claim can join a set whose newest is in time.
    let mut fixture = Fixture::new();
    let old = fixture.row(day(8, 1), -1000, "Hotel", None);
    let recent = fixture.row(day(11, 1), -2000, "Train", None);
    for id in [old, recent] {
        set_sharing(&mut fixture.db, id, &Sharing::ClaimBack).unwrap();
    }
    let paid = fixture.row(day(11, 9), 3000, "ACME", None);
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 2);
    assert_eq!(links_of(&fixture.db, paid).unwrap().len(), 2);
}

#[test]
fn suggest_for_batch_looks_at_the_batchs_money_in_only() {
    let fixture = Fixture::new();
    let purchase = fixture.row(day(11, 1), -1000, "Shop", None);
    let manual_refund = fixture.row(day(11, 2), 1000, "Shop", None);
    fixture
        .conn()
        .execute(
            "INSERT INTO import_batch (account_id, source_file, format) \
             VALUES (?1, 'statement.csv', 'csv')",
            [fixture.account],
        )
        .unwrap();
    let batch = BatchId::new(fixture.conn().last_insert_rowid());
    fixture
        .conn()
        .execute(
            "INSERT INTO \"transaction\" (account_id, batch_id, date, amount_minor, currency, \
             merchant_display, description_raw, description_normalized, fingerprint, origin) \
             VALUES (?1, ?2, '2026-11-03', 500, 'GBP', 'Shop', 'Shop', 'SHOP', 'fp-1', \
             'imported')",
            params![fixture.account, batch],
        )
        .unwrap();
    let imported = TransactionId::new(fixture.conn().last_insert_rowid());

    assert_eq!(suggest_for_batch(fixture.conn(), batch).unwrap(), 1);
    assert_eq!(links_of(&fixture.db, imported).unwrap()[0].amount, gbp(500));
    assert!(links_of(&fixture.db, manual_refund).unwrap().is_empty());

    // Undo clears the batch's links, then its rows can go.
    let delete = "DELETE FROM \"transaction\" WHERE batch_id = ?1";
    assert!(fixture.conn().execute(delete, [batch]).is_err());
    assert_eq!(clear_batch(fixture.conn(), batch).unwrap(), 1);
    assert_eq!(fixture.conn().execute(delete, [batch]).unwrap(), 1);
    let _ = purchase;
}

// Clearing --------------------------------------------------------------------

#[test]
fn deleting_a_manual_row_clears_its_links() {
    let mut fixture = Fixture::new();
    let purchase = fixture.row(day(11, 1), -1000, "Shop", Some("shopping"));
    let refund = fixture.row(day(11, 2), 1000, "Shop", Some("income"));
    fixture.refund(refund, purchase, 1000);

    manual::delete(&mut fixture.db, purchase).unwrap();
    assert!(fixture.statuses().is_empty());
    assert_eq!(
        fixture.ledger(refund),
        [(1000, s("income"), 1000, 1000, None, true)]
    );
}

#[test]
fn a_linked_manual_rows_amount_is_fixed() {
    let mut fixture = Fixture::new();
    let purchase = fixture.row(day(11, 1), -1000, "Shop", None);
    let refund = fixture.row(day(11, 2), 1000, "Shop", None);
    let id = fixture.refund(refund, purchase, 1000);
    let mut entry = manual::get(&fixture.db, purchase).unwrap();
    entry.amount = gbp(-900);
    assert!(matches!(
        manual::update(&mut fixture.db, purchase, &entry),
        Err(manual::Error::Linked(row)) if row == purchase
    ));
    // Other fields stay editable.
    let mut renamed = manual::get(&fixture.db, purchase).unwrap();
    renamed.description = "Shop again".to_owned();
    manual::update(&mut fixture.db, purchase, &renamed).unwrap();

    // Once unlinked, the amount can change; a suggestion doesn't stop it.
    reject(&mut fixture.db, id).unwrap();
    let other = fixture.row(day(11, 3), 900, "Shop", None);
    assert_eq!(suggest_all(fixture.conn()).unwrap(), 1);
    manual::update(&mut fixture.db, purchase, &entry).unwrap();
    assert!(links_of(&fixture.db, other).unwrap().is_empty());
}

#[test]
fn errors_dont_quote_labels() {
    let mut fixture = Fixture::new();
    let purchase = fixture.row(day(11, 7), -3180, "Odeon Secret", None);
    let inflow = fixture.row(day(11, 8), 5000, "Odeon Secret", None);
    let message = create(
        &mut fixture.db,
        &NewLink {
            kind: LinkKind::Settlement,
            inflow,
            target: LinkTarget::Person(label("Sam Secret")),
            amount: gbp(100),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(!message.contains("Secret"), "{message}");
    let message = set_sharing(
        &mut fixture.db,
        purchase,
        &Sharing::Shared {
            my_share: gbp(-4000),
            label: label("Sam Secret"),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(!message.contains("Secret"), "{message}");
}
