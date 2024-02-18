use std::io::Write;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::json;

pub struct FullName(serde_json::Map<String, serde_json::Value>);

impl FullName {
    pub fn new(
        name: impl ToString,
        labels: impl IntoIterator<Item = (impl ToString, impl ToString)>,
    ) -> Self {
        let labels = labels.into_iter();
        let mut map = serde_json::Map::with_capacity(labels.size_hint().1.unwrap_or(0) + 1);
        for (k, v) in labels {
            let k = k.to_string();
            assert_ne!(k, "__name__");
            map.insert(k, json!(v.to_string()));
        }

        map.insert("__name__".to_string(), json!(name.to_string()));

        FullName(map)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Obs {
    value: f64,
    timestamp: i64,
}

impl Obs {
    pub fn new(value: f64, when: DateTime<Utc>) -> Self {
        Obs {
            value,
            timestamp: when.timestamp_millis(),
        }
    }

    pub fn now(value: f64) -> Self {
        Obs::new(value, Utc::now())
    }
}

pub fn write_metric(mut write: impl Write, name: &FullName, obs: &[Obs]) -> Result<()> {
    let values = obs.iter().map(|o| o.value).collect::<Vec<_>>();
    let timestamps = obs.iter().map(|o| o.timestamp).collect::<Vec<_>>();

    serde_json::to_writer(
        &mut write,
        &json!({
            "metric": name.0,
            "values": values,
            "timestamps": timestamps,
        }),
    )?;
    write.write_all(b"\n")?;

    Ok(())
}
