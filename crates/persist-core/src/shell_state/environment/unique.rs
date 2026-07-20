use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::de::{Error, MapAccess, SeqAccess, Visitor};
use serde::Deserializer;

pub(super) fn map<'de, D>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_map(UniqueMapVisitor)
}

pub(super) fn set<'de, D>(deserializer: D) -> Result<BTreeSet<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_seq(UniqueSetVisitor)
}

struct UniqueMapVisitor;

impl<'de> Visitor<'de> for UniqueMapVisitor {
    type Value = BTreeMap<String, String>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an environment object with unique names")
    }

    fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = BTreeMap::new();
        while let Some((name, value)) = access.next_entry()? {
            if values.insert(name, value).is_some() {
                return Err(A::Error::custom("duplicate environment name"));
            }
        }
        Ok(values)
    }
}

struct UniqueSetVisitor;

impl<'de> Visitor<'de> for UniqueSetVisitor {
    type Value = BTreeSet<String>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an environment name array with unique entries")
    }

    fn visit_seq<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = BTreeSet::new();
        while let Some(name) = access.next_element::<String>()? {
            if !values.insert(name) {
                return Err(A::Error::custom("duplicate environment name"));
            }
        }
        Ok(values)
    }
}
