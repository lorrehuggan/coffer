//! Row IDs, one type per table, so an account ID can't be passed where a
//! batch ID is meant.
//!
//! IDs are SQLite `INTEGER PRIMARY KEY AUTOINCREMENT` values: never reused
//! after a delete (an undone import), so an ID kept anywhere can't come to
//! name a different row.

use std::fmt;

use rusqlite::ToSql;
use rusqlite::types::{FromSql, FromSqlResult, ToSqlOutput, ValueRef};

macro_rules! row_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
            serde::Serialize, serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(i64);

        impl $name {
            /// Wraps a stored ID, e.g. one the user typed (`coffer
            /// undo-import 12`). Whether the row exists is the database's
            /// question.
            pub const fn new(id: i64) -> Self {
                Self(id)
            }

            pub const fn get(self) -> i64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                self.0.to_sql()
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                i64::column_result(value).map(Self)
            }
        }
    };
}

row_id!(
    /// An `account` row.
    AccountId
);
row_id!(
    /// An `import_batch` row.
    BatchId
);
row_id!(
    /// A `"transaction"` row.
    TransactionId
);
row_id!(
    /// A `transaction_split` row: one line of a split transaction.
    SplitId
);
row_id!(
    /// A `balance_snapshot` row.
    BalanceId
);
row_id!(
    /// A `transfer_group` row.
    TransferGroupId
);
row_id!(
    /// A `rule` row.
    RuleId
);
row_id!(
    /// A `tag` row.
    TagId
);
row_id!(
    /// A `merchant_alias` row.
    MerchantAliasId
);
row_id!(
    /// A `change_batch` row.
    ChangeBatchId
);
row_id!(
    /// A `transaction_link` row: a refund, reimbursement or settlement.
    LinkId
);
