//! What the Settings screens show, read from the database (or sample data):
//! plain values, with every time already in the computer's time zone.

use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use coffer_core::access_log::LoggedCall;
use coffer_core::access_log::activity::{ClientActivity, Written};
use coffer_core::backup::Backup;
use coffer_core::backup::detail::{Contents, Detail};
use coffer_core::db::SchemaVersion;
use coffer_core::id::AccountId;
use coffer_core::mcp_client::{AccessLevel, ClientName};

/// When "now" is, for "last 7 days" and "today".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub now: DateTime<Utc>,
    /// Today in the computer's time zone.
    pub today: NaiveDate,
}

/// A client the user has enabled (it may be turned off since).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientRow {
    pub name: ClientName,
    pub enabled: bool,
    /// Its level: kept while it's turned off.
    pub level: AccessLevel,
    /// Its latest call or request, local time.
    pub last_seen: Option<NaiveDateTime>,
    /// What it did in the last 7 days.
    pub week: ClientActivity,
    /// What it left in the Inbox in the last 7 days.
    pub written: Written,
}

/// A client that asked to connect and hasn't been answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestRow {
    pub name: ClientName,
    /// Its latest refused call (or its request), local time.
    pub last_tried: Option<NaiveDateTime>,
    /// Its refused calls in the log (12 months).
    pub refused: u64,
}

/// The AI clients screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AiData {
    /// The kill switch: AI access on.
    pub on: bool,
    pub clients: Vec<ClientRow>,
    pub requests: Vec<RequestRow>,
    /// Calls in the last 7 days, every client: the panel's Access log count.
    pub calls_week: u64,
    /// Every client name in the log, for the log's client filter.
    pub log_clients: Vec<String>,
}

impl AiData {
    pub fn client(&self, name: &ClientName) -> Option<&ClientRow> {
        self.clients.iter().find(|client| &client.name == name)
    }

    pub fn request(&self, name: &ClientName) -> Option<&RequestRow> {
        self.requests.iter().find(|request| &request.name == name)
    }

    /// The clients enabled now (the status bar's "N agents connected" counts
    /// them while AI access is on).
    pub fn enabled(&self) -> usize {
        self.clients.iter().filter(|client| client.enabled).count()
    }
}

/// How far back the access log goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Period {
    Today,
    #[default]
    Week,
    Month,
    /// Everything kept: 12 months.
    All,
}

impl Period {
    pub const ALL: [Period; 4] = [Period::Today, Period::Week, Period::Month, Period::All];

    pub fn key(self) -> &'static str {
        match self {
            Period::Today => "today",
            Period::Week => "week",
            Period::Month => "month",
            Period::All => "all",
        }
    }

    pub fn from_key(key: &str) -> Option<Period> {
        Period::ALL.into_iter().find(|period| period.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Period::Today => "Today",
            Period::Week => "Last 7 days",
            Period::Month => "Last 30 days",
            Period::All => "Last 12 months",
        }
    }
}

/// Which calls the access log shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogFilter {
    /// One client's calls, `None` for all.
    pub client: Option<String>,
    pub period: Period,
}

/// A logged call as the access log shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRow {
    pub call: LoggedCall,
    /// When, local time.
    pub local: NaiveDateTime,
    /// Whether its tool writes to the Inbox.
    pub writes: bool,
}

/// A page of the access log.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogPage {
    pub rows: Vec<LogRow>,
    /// Whether older calls match the filter.
    pub more: bool,
}

/// A backup as the Backups screen lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupRow {
    pub backup: Backup,
    /// When it was taken, local time.
    pub local: NaiveDateTime,
    /// What it holds, `None` when it couldn't be read.
    pub contents: Option<Contents>,
    pub detail: Detail,
    /// Listed under "One a month" rather than "Recent".
    pub monthly: bool,
}

/// The Backups screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackupsData {
    pub rows: Vec<BackupRow>,
    /// This build's schema version.
    pub supported: SchemaVersion,
}

impl BackupsData {
    pub fn row(&self, path: &PathBuf) -> Option<&BackupRow> {
        self.rows.iter().find(|row| &row.backup.path == path)
    }
}

/// What the export dialog's selects offer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportChoices {
    /// Open accounts, by name.
    pub accounts: Vec<(AccountId, String)>,
    /// Categories in tree order: key, label, and whether it's a group.
    pub categories: Vec<(String, String, bool)>,
    /// Tags by name, and whether each is private.
    pub tags: Vec<(String, bool)>,
    /// The ledger's first and last dates.
    pub span: Option<(NaiveDate, NaiveDate)>,
    pub today: Option<NaiveDate>,
}
