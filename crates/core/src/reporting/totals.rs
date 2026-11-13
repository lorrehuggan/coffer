//! Per-currency totals ([`Totals`]) and the row cap ([`Limit`], [`Capped`]).

use std::collections::BTreeMap;

use crate::money::{Currency, Money};

use super::Error;

/// Sums of money, one per currency. Amounts in different currencies are
/// never added together: a report shows one total per currency until
/// home-currency conversion exists (M4.6).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    by_currency: BTreeMap<Currency, Money>,
}

impl Totals {
    /// No totals: no currency at all, not zero in some currency.
    pub fn new() -> Totals {
        Totals::default()
    }

    /// Adds `money` to its currency's total (starting it at zero). Fails
    /// only when the total overflows `i64`.
    pub fn add(&mut self, money: Money) -> Result<(), Error> {
        let total = self
            .by_currency
            .entry(money.currency())
            .or_insert_with(|| Money::zero(money.currency()));
        *total = total.checked_add(money)?;
        Ok(())
    }

    /// Adds every total of `other`, currency by currency.
    pub fn add_all(&mut self, other: &Totals) -> Result<(), Error> {
        other.iter().try_for_each(|money| self.add(money))
    }

    /// The total in `currency`, `None` when nothing in it was added.
    pub fn get(&self, currency: Currency) -> Option<Money> {
        self.by_currency.get(&currency).copied()
    }

    /// Every total, ordered by currency code.
    pub fn iter(&self) -> impl Iterator<Item = Money> + '_ {
        self.by_currency.values().copied()
    }

    /// The currencies with a total, ordered by code.
    pub fn currencies(&self) -> impl Iterator<Item = Currency> + '_ {
        self.by_currency.keys().copied()
    }

    /// Every total with its sign flipped, e.g. spending (money out, below
    /// zero) shown as a positive amount spent.
    pub fn negated(&self) -> Result<Totals, Error> {
        let mut negated = Totals::new();
        for money in self.iter() {
            negated.add(money.checked_neg()?)?;
        }
        Ok(negated)
    }

    /// No currency has a total.
    pub fn is_empty(&self) -> bool {
        self.by_currency.is_empty()
    }

    /// How many currencies have a total.
    pub fn len(&self) -> usize {
        self.by_currency.len()
    }
}

/// The most rows one call returns: transactions, months, accounts or any
/// other list.
pub const MAX_ROWS: usize = 200;

/// How many rows a call returns when the caller doesn't say.
pub const DEFAULT_ROWS: usize = 50;

/// How many rows a call may return: from 1 to [`MAX_ROWS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Limit(usize);

impl Limit {
    /// The most a call returns.
    pub const MAX: Limit = Limit(MAX_ROWS);

    /// The caller's limit, clamped to 1–[`MAX_ROWS`]; [`DEFAULT_ROWS`]
    /// when there is none.
    pub fn new(requested: Option<u32>) -> Limit {
        let requested = requested.map_or(DEFAULT_ROWS, |n| n as usize);
        Limit(requested.clamp(1, MAX_ROWS))
    }

    pub fn get(self) -> usize {
        self.0
    }
}

/// A list cut at a [`Limit`]: `truncated` says whether more rows matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capped<T> {
    pub items: Vec<T>,
    pub truncated: bool,
}

impl<T> Capped<T> {
    /// The first `limit` items of `items`.
    pub fn new(items: impl IntoIterator<Item = T>, limit: Limit) -> Capped<T> {
        let mut items = items.into_iter();
        let kept: Vec<T> = items.by_ref().take(limit.get()).collect();
        let truncated = items.next().is_some();
        Capped {
            items: kept,
            truncated,
        }
    }

    pub fn map<U>(self, f: impl FnMut(T) -> U) -> Capped<U> {
        Capped {
            items: self.items.into_iter().map(f).collect(),
            truncated: self.truncated,
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_are_kept_per_currency() {
        let mut totals = Totals::new();
        assert!(totals.is_empty());
        totals.add(Money::new(-500, Currency::GBP)).unwrap();
        totals.add(Money::new(-700, Currency::EUR)).unwrap();
        totals.add(Money::new(200, Currency::GBP)).unwrap();
        assert_eq!(
            totals.get(Currency::GBP),
            Some(Money::new(-300, Currency::GBP))
        );
        assert_eq!(
            totals.get(Currency::EUR),
            Some(Money::new(-700, Currency::EUR))
        );
        assert_eq!(totals.get(Currency::USD), None);
        let codes: Vec<String> = totals.currencies().map(|c| c.code().to_owned()).collect();
        assert_eq!(codes, ["EUR", "GBP"]);
        let negated = totals.negated().unwrap();
        assert_eq!(
            negated.get(Currency::GBP),
            Some(Money::new(300, Currency::GBP))
        );

        let mut all = Totals::new();
        all.add_all(&totals).unwrap();
        all.add_all(&negated).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|money| money.is_zero()));
    }

    #[test]
    fn totals_refuse_to_overflow() {
        let mut totals = Totals::new();
        totals.add(Money::new(i64::MAX, Currency::GBP)).unwrap();
        assert!(matches!(
            totals.add(Money::new(1, Currency::GBP)),
            Err(Error::Money(_))
        ));
    }

    #[test]
    fn limits_are_clamped_and_capped_lists_say_when_they_are_cut() {
        assert_eq!(Limit::new(None).get(), DEFAULT_ROWS);
        assert_eq!(Limit::new(Some(0)).get(), 1);
        assert_eq!(Limit::new(Some(10)).get(), 10);
        assert_eq!(Limit::new(Some(10_000)).get(), MAX_ROWS);

        let all = Capped::new(0..3, Limit::new(Some(3)));
        assert_eq!((all.items, all.truncated), (vec![0, 1, 2], false));
        let cut = Capped::new(0..4, Limit::new(Some(3))).map(|n| n * 2);
        assert_eq!((cut.items, cut.truncated), (vec![0, 2, 4], true));
    }
}
