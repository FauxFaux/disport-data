use std::collections::HashMap;
use std::env;

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as b64;
use base64::Engine;
use chrono::Utc;
use convert_case::{Case, Casing};
use hmac::Mac;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

type HmacSha1 = hmac::Hmac<sha1::Sha1>;

pub fn load_config() -> Result<Req> {
    Ok(Req {
        api: env_var("SOLIS_API")?.trim_end_matches('/').to_string(),
        key: env_var("SOLIS_KEY")?,
        secret: env_var("SOLIS_SECRET")?,
    })
}

pub struct SolisCloud {
    config: Req,
    inverter_ids: Vec<String>,
}

pub async fn warmup(http: &Client, config: &Req) -> Result<SolisCloud> {
    let resp = call_api::<Resp<AllInverters>>(
        &http,
        &config,
        "/v1/api/inverterList",
        &json!({
                "pageNo": 1,
                "pageSize": 10,
        }),
    )
    .await?;

    let inverter_ids = resp
        .data
        .page
        .records
        .iter()
        .map(|i| i.id.clone())
        .collect::<Vec<_>>();

    Ok(SolisCloud {
        config: config.clone(),
        inverter_ids,
    })
}

pub async fn run(http: &Client, solis: &SolisCloud) -> Result<()> {
    for id in &solis.inverter_ids {
        let resp = call_api::<Resp<HashMap<String, Value>>>(
            &http,
            &solis.config,
            "/v1/api/inverterDetail",
            &json!({
                "id": id,
            }),
        )
        .await?;

        let mapped = map_detail(&resp.data)?;
    }

    Ok(())
}

#[derive(Clone)]
pub struct Req {
    api: String,
    key: String,
    secret: String,
}

#[derive(Deserialize)]
struct Resp<T> {
    code: String,
    msg: String,
    data: T,
    success: bool,
}

#[derive(Deserialize)]
struct AllInverters {
    page: Pager<InverterLite>,
    // incomplete
}

#[derive(Deserialize)]
struct InverterLite {
    id: String,
    sn: String,
    // incomplete
}

#[derive(Deserialize)]
struct Pager<T> {
    records: Vec<T>,
    total: i64,
    // incomplete
}

#[derive(Deserialize, Debug)]
struct Station {
    sno: String,
    id: String,
}

async fn call_api<T: DeserializeOwned>(
    client: &reqwest::Client,
    req: &Req,
    path: &str,
    data: &impl Serialize,
) -> Result<T> {
    let data = serde_json::to_vec(&data)?;
    let md5 = b64.encode(md5::compute(&data).0);
    // TODO: +0000 instead of 'GMT'? Doesn't seem to care
    let now = Utc::now().to_rfc2822();
    let param = format!("POST\n{md5}\napplication/json\n{now}\n{path}");
    let mut mac = HmacSha1::new_from_slice(req.secret.as_bytes())?;
    mac.update(param.as_bytes());
    let signature = b64.encode(mac.finalize().into_bytes());
    let resp = client
        .post(format!("{}{path}", req.api))
        .header("Content-Type", "application/json;charset=utf-8")
        .header("Date", now)
        .header("Authorization", format!("API {}:{signature}", req.key))
        .header("Content-MD5", md5)
        .body(data)
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.json::<T>().await?)
}

fn env_var(name: &'static str) -> Result<String> {
    env::var(name).with_context(|| anyhow!("reading env var {name:?}"))
}

fn map_detail(detail: &HashMap<String, Value>) -> Result<HashMap<String, String>> {
    let mut m = HashMap::with_capacity(100);
    let mut rem = detail.clone();

    let to_str = |v: Value| match v {
        Value::String(s) => s,
        other => other.to_string(),
    };

    for (unit_field, unit) in detail.iter() {
        if !unit_field.ends_with("Str") {
            continue;
        }
        if unit_field.contains("Time") {
            continue;
        }
        let real_field = unit_field.trim_end_matches("Str");
        let Some(unit) = unit.as_str() else { continue };
        let Some(real_value) = rem.remove(real_field) else {
            continue;
        };
        let unit = unit.replace(|c: char| !c.is_ascii_alphanumeric(), "");
        if unit.is_empty() {
            continue;
        }

        let (mul, unit) = match unit.as_str() {
            "Wh" => (0.001, "kWh"),
            "MWh" => (1_000., "kWh"),
            "GWh" => (1_000_000., "kWh"),
            other => (1., other),
        };

        let real_value = match real_value {
            Value::Number(n) => {
                json!(n.as_f64().expect("all numbers are f64 without features") * mul)
            }
            other => other,
        };

        rem.remove(unit_field);
        m.insert(
            format!(
                "{}_{}",
                real_field.to_case(Case::Snake),
                unit.to_ascii_lowercase()
            ),
            to_str(real_value),
        );
    }

    for (k, v) in rem {
        m.insert(k.to_case(Case::Snake), to_str(v));
    }

    Ok(m)
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    #[test]
    fn test_detail() -> Result<()> {
        let m = super::map_detail(&serde_json::from_str(include_str!(
            "../tests/ref/soliscloud/inverterDetail.json"
        ))?)?;
        assert_eq!(
            m.get("home_load_today_energy_kwh"),
            Some(&"6.1".to_string())
        );
        Ok(())
    }

    #[test]
    fn test_unit_flip() -> Result<()> {
        let m = super::map_detail(&serde_json::from_str(include_str!(
            "../tests/ref/soliscloud/unitFlip.json"
        ))?)?;
        assert_eq!(
            m.get("home_load_total_energy_kwh"),
            Some(&"1377.0".to_string())
        );
        assert_eq!(
            m.get("home_load_yesterday_energy_kwh"),
            Some(&"12.9".to_string())
        );
        Ok(())
    }

    #[test]
    fn test_to_string() {
        assert_eq!("\"hello\"", serde_json::json!("hello").to_string());
        assert_eq!("0.0", serde_json::json!(0.).to_string());
        assert_eq!("0", serde_json::json!(0).to_string());
    }
}
