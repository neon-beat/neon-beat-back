use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::dao::models::SongEntity;

pub fn serialize<S>(value: &HashMap<u32, SongEntity>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let as_string = value
        .iter()
        .map(|(key, song)| (key.to_string(), song))
        .collect::<HashMap<_, _>>();
    as_string.serialize(serializer)
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<u32, SongEntity>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = HashMap::<String, SongEntity>::deserialize(deserializer)?;
    raw.into_iter()
        .map(|(key, song)| {
            key.parse::<u32>()
                .map(|parsed| (parsed, song))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}
