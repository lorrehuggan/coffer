//! Category keys: the stable names presets, rules and MCP use for
//! categories.
//!
//! A category has a translatable label for people and a fixed key for
//! everything else (`category.key` in the schema). Keys are one or two
//! segments of lowercase ASCII letters, digits and `_`, joined by a dot:
//! `transfer`, `food.groceries`. The second segment is a subcategory of the
//! first, matching the two-level category tree.

use std::fmt;
use std::str::FromStr;

/// A category's stable key, e.g. `food.groceries`.
///
/// Only the spelling is checked here. Whether a category with this key
/// exists is the database's question.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CategoryKey(String);

/// Text that isn't a valid category key. Keys come from presets and rules,
/// not from transactions, so the text is kept for the message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "{0:?} isn't a category key: use one or two parts of lowercase letters, digits and `_`, \
     joined by a dot (`food.groceries`)"
)]
pub struct CategoryKeyError(pub String);

impl CategoryKey {
    pub fn new(key: &str) -> Result<CategoryKey, CategoryKeyError> {
        let segment_ok = |segment: &str| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        };
        let mut segments = key.split('.');
        let valid = segments.next().is_some_and(segment_ok)
            && segments.next().is_none_or(segment_ok)
            && segments.next().is_none();
        if valid {
            Ok(CategoryKey(key.to_owned()))
        } else {
            Err(CategoryKeyError(key.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The top-level key of a subcategory (`food` for `food.groceries`),
    /// `None` for a top-level key.
    pub fn parent(&self) -> Option<&str> {
        self.0.split_once('.').map(|(parent, _)| parent)
    }
}

impl fmt::Display for CategoryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for CategoryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CategoryKey({})", self.0)
    }
}

impl FromStr for CategoryKey {
    type Err = CategoryKeyError;

    fn from_str(key: &str) -> Result<CategoryKey, CategoryKeyError> {
        CategoryKey::new(key)
    }
}

impl serde::Serialize for CategoryKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for CategoryKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<CategoryKey, D::Error> {
        let key = String::deserialize(deserializer)?;
        CategoryKey::new(&key).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_one_or_two_segments() {
        for key in [
            "transfer",
            "food.groceries",
            "food.eating_out",
            "bills2.tv_licence",
        ] {
            assert_eq!(CategoryKey::new(key).unwrap().as_str(), key);
        }
    }

    #[test]
    fn refuses_anything_else() {
        for key in [
            "",
            ".",
            "food.",
            ".groceries",
            "food..groceries",
            "a.b.c",
            "Food",
            "food.Groceries",
            "food groceries",
            "food-groceries",
            " food",
            "café",
        ] {
            assert_eq!(
                CategoryKey::new(key),
                Err(CategoryKeyError(key.to_owned())),
                "{key:?}"
            );
        }
    }

    #[test]
    fn parent_is_the_first_segment_of_a_subcategory() {
        assert_eq!(
            CategoryKey::new("food.groceries").unwrap().parent(),
            Some("food")
        );
        assert_eq!(CategoryKey::new("transfer").unwrap().parent(), None);
    }

    #[test]
    fn serde_uses_the_text_and_validates_it() {
        let key: CategoryKey = serde_json::from_str(r#""food.groceries""#).unwrap();
        assert_eq!(serde_json::to_string(&key).unwrap(), r#""food.groceries""#);
        assert!(serde_json::from_str::<CategoryKey>(r#""Food""#).is_err());
    }
}
