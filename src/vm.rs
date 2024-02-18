use std::io::Write;

use anyhow::Result;
use serde_json::json;

pub struct FullName {
    name: String,
    labels: Box<[(String, String)]>,
}

impl FullName {
    pub fn new(
        name: impl ToString,
        labels: impl IntoIterator<Item = (impl ToString, impl ToString)>,
    ) -> Self {
        FullName {
            name: name.to_string(),
            labels: labels
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Obs {
    value: f64,
    timestamp: i64,
}

impl Obs {
    pub fn now(value: f64) -> Self {
        Obs {
            value,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }
}

pub fn write_metric(mut write: impl Write, name: &FullName, obs: &[Obs]) -> Result<()> {
    let mut metric = serde_json::Map::with_capacity(name.labels.len() + 1);
    for (k, v) in name.labels.as_ref() {
        metric.insert(k.clone(), serde_json::Value::String(v.clone()));
    }

    metric.insert(
        "__name__".to_string(),
        serde_json::Value::String(name.name.clone()),
    );

    let values = obs.iter().map(|o| json!(o.value)).collect::<Vec<_>>();
    let timestamps = obs.iter().map(|o| json!(o.timestamp)).collect::<Vec<_>>();

    serde_json::to_writer(
        &mut write,
        &json!({
            "metric": metric,
            "values": values,
            "timestamps": timestamps,
        }),
    )?;
    write.write_all(b"\n")?;

    Ok(())
}
