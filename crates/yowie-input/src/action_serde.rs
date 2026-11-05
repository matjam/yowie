//! Saving and loading action maps.
//!
//! A map is written as an object keyed by action name, in creation order.
//! A one-axis action is a list of binding strings; a two-axis action is an
//! object with a `vec2` list of sources and directions:
//!
//! ```json
//! {
//!   "jump": ["key:Space", "pad:South"],
//!   "move": {"vec2": [{"source": "key:W", "direction": [0.0, -1.0]}]}
//! }
//! ```

use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use yowie_core::Vec2;

use crate::action_map::Entry;
use crate::{ActionMap, Binding, Binding2, Bindings};

impl Serialize for Binding {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Binding {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(de::Error::custom)
    }
}

#[derive(Serialize, Deserialize)]
struct StoredDirection {
    source: String,
    direction: [f32; 2],
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum StoredBindings {
    Axis(Vec<Binding>),
    Vec2 { vec2: Vec<StoredDirection> },
}

impl StoredBindings {
    fn from_bindings(b: &Bindings) -> Self {
        match b {
            Bindings::Axis(list) => StoredBindings::Axis(list.clone()),
            Bindings::Vec2(list) => StoredBindings::Vec2 {
                vec2: list
                    .iter()
                    .map(|b| StoredDirection {
                        source: b.source.to_string(),
                        direction: b.direction.to_array(),
                    })
                    .collect(),
            },
        }
    }

    fn into_bindings<E: de::Error>(self) -> Result<Bindings, E> {
        Ok(match self {
            StoredBindings::Axis(list) => Bindings::Axis(list),
            StoredBindings::Vec2 { vec2 } => Bindings::Vec2(
                vec2.into_iter()
                    .map(|d| {
                        Ok(Binding2 {
                            source: d.source.parse().map_err(E::custom)?,
                            direction: Vec2::from_array(d.direction),
                        })
                    })
                    .collect::<Result<_, E>>()?,
            ),
        })
    }
}

impl Serialize for ActionMap {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(self.entries.len()))?;
        for e in &self.entries {
            map.serialize_entry(&e.name, &StoredBindings::from_bindings(&e.bindings))?;
        }
        map.end()
    }
}

struct MapVisitor;

impl<'de> Visitor<'de> for MapVisitor {
    type Value = ActionMap;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a map from action names to bindings")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<ActionMap, A::Error> {
        let mut out = ActionMap::new();
        while let Some((name, stored)) = access.next_entry::<String, StoredBindings>()? {
            let i = out.action(&name).0 as usize;
            out.entries[i] = Entry {
                name,
                bindings: stored.into_bindings()?,
            };
        }
        Ok(out)
    }
}

impl<'de> Deserialize<'de> for ActionMap {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_map(MapVisitor)
    }
}
