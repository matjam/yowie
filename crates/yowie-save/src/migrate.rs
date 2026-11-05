use serde_json::Value;

use crate::SaveError;

/// A hook that upgrades save data by one version.
///
/// It receives the version the data is at and the data, and returns the
/// data at the next version. The store calls it repeatedly until the data
/// reaches the store's current version. Return `Err(reason)` to refuse a
/// save.
pub type Migration = Box<dyn Fn(u32, Value) -> Result<Value, String> + Send + Sync>;

/// Splits file contents into `(version, data)`. A file that is not a
/// `{"version", "data"}` envelope is legacy data at version 0.
pub(crate) fn open_envelope(value: Value) -> (u32, Value) {
    if let Value::Object(map) = &value
        && map.len() == 2
        && let (Some(Value::Number(v)), true) = (map.get("version"), map.contains_key("data"))
        && let Some(version) = v.as_u64().and_then(|v| u32::try_from(v).ok())
    {
        let data = match value {
            Value::Object(mut m) => m.remove("data").unwrap_or(Value::Null),
            _ => Value::Null,
        };
        return (version, data);
    }
    (0, value)
}

/// Upgrades `data` from `version` to `current` with `migration`.
pub(crate) fn upgrade(
    name: &str,
    mut version: u32,
    mut data: Value,
    current: u32,
    migration: Option<&Migration>,
) -> Result<Value, SaveError> {
    if version > current {
        return Err(SaveError::TooNew {
            name: name.to_string(),
            found: version,
            supported: current,
        });
    }
    while version < current {
        let Some(step) = migration else {
            // Without a hook the data is taken as it is.
            break;
        };
        data = step(version, data).map_err(|reason| SaveError::Migration {
            name: name.to_string(),
            from: version,
            reason,
        })?;
        version += 1;
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn envelope_detection() {
        let cases = [
            (json!({"version": 3, "data": {"a": 1}}), 3, json!({"a": 1})),
            (json!({"a": 1}), 0, json!({"a": 1})),
            (
                json!({"version": 3, "data": 1, "extra": 2}),
                0,
                json!({"version": 3, "data": 1, "extra": 2}),
            ),
            (json!(5), 0, json!(5)),
            (
                json!({"version": -1, "data": 1}),
                0,
                json!({"version": -1, "data": 1}),
            ),
        ];
        for (input, version, data) in cases {
            assert_eq!(open_envelope(input.clone()), (version, data), "{input}");
        }
    }

    #[test]
    fn upgrade_steps_and_errors() {
        let add_one: Migration =
            Box::new(|from, v| Ok(json!(v.as_i64().unwrap_or(0) + i64::from(from) + 1)));
        assert_eq!(
            upgrade("s", 0, json!(0), 3, Some(&add_one)).unwrap(),
            json!(6)
        );
        assert_eq!(
            upgrade("s", 2, json!(10), 2, Some(&add_one)).unwrap(),
            json!(10)
        );
        assert_eq!(upgrade("s", 0, json!(7), 2, None).unwrap(), json!(7));
        assert!(matches!(
            upgrade("s", 5, json!(0), 2, None),
            Err(SaveError::TooNew {
                found: 5,
                supported: 2,
                ..
            })
        ));
        let fail: Migration = Box::new(|_, _| Err("nope".into()));
        assert!(matches!(
            upgrade("s", 0, json!(0), 1, Some(&fail)),
            Err(SaveError::Migration { from: 0, .. })
        ));
    }
}
