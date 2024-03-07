use std::collections::HashMap;
use std::env;
use std::hash::Hash;

use anyhow::{anyhow, bail, ensure, Context, Result};
use base64::engine::general_purpose::STANDARD as b64;
use base64::Engine;
use chrono::{TimeDelta, TimeZone, Utc};
use convert_case::{Case, Casing};
use hmac::Mac;
use log::warn;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::Solis;
use crate::vm::{FullName, Obs};

type HmacSha1 = hmac::Hmac<sha1::Sha1>;

pub struct Service {
    config: Solis,
    inverter_ids: Vec<String>,
}

pub async fn warmup(http: &Client, config: Solis) -> Result<Service> {
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

    Ok(Service {
        config,
        inverter_ids,
    })
}

pub async fn run(http: &Client, solis: &Service) -> Result<Vec<(FullName, Obs)>> {
    let mut ret = Vec::with_capacity(300);
    for id in &solis.inverter_ids {
        let mut resp = call_api::<Resp<HashMap<String, Value>>>(
            &http,
            &solis.config,
            "/v1/api/inverterDetail",
            &json!({
                "id": id,
            }),
        )
        .await?;

        let ts = match resp.data.remove("dataTimestamp") {
            Some(Value::String(s)) => s.parse::<i64>().ok(),
            Some(Value::Number(n)) => n.as_i64(),
            _ => None,
        }
        .and_then(|n| Utc.timestamp_millis_opt(n).single());

        let ts = match ts {
            Some(ts) if (ts - Utc::now()).abs() < TimeDelta::minutes(10) => Some(ts),
            Some(ts) => {
                warn!("timestamp {ts:?} too far from now");
                None
            }
            None => {
                warn!("no timestamp in response");
                None
            }
        }
        .unwrap_or_else(Utc::now);

        for (k, v) in map(&resp.data)? {
            ret.push((
                FullName::new(format!("soliscloud_{k}"), [("id", id)]),
                Obs::new(v, ts),
            ));
        }
    }

    Ok(ret)
}

pub fn map(detail: &HashMap<String, Value>) -> Result<HashMap<String, f64>> {
    let (good, bad) = opinionated(detail)?;
    let mut ret = HashMap::with_capacity(good.len() + bad.len());
    for (k, v) in good {
        ret.insert(format!("soliscloud_{k}"), v);
    }

    let bad = map_detail(&bad)?;

    for (k, v) in bad {
        if let Some(v) = v.parse::<f64>().ok() {
            ret.insert(format!("soliscloud_raw_{k}"), v);
        }
    }

    Ok(ret)
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
    http: &Client,
    cfg: &Solis,
    path: &str,
    data: &impl Serialize,
) -> Result<T> {
    let data = serde_json::to_vec(&data)?;
    let md5 = b64.encode(md5::compute(&data).0);
    // TODO: +0000 instead of 'GMT'? Doesn't seem to care
    let now = Utc::now().to_rfc2822();
    let param = format!("POST\n{md5}\napplication/json\n{now}\n{path}");
    let mut mac = HmacSha1::new_from_slice(cfg.secret.as_bytes())?;
    mac.update(param.as_bytes());
    let signature = b64.encode(mac.finalize().into_bytes());
    let resp = http
        .post(format!("{}{path}", cfg.api))
        .header("Content-Type", "application/json;charset=utf-8")
        .header("Date", now)
        .header("Authorization", format!("API {}:{signature}", cfg.key))
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

fn opinionated(
    detail: &HashMap<String, Value>,
) -> Result<(HashMap<String, f64>, HashMap<String, Value>)> {
    let mut rem = detail.clone();

    // leaving just 0 (the primary / only mppt string)
    for i in 1..40 {
        for k in &[
            "iPv", "mpptIpv", "mpptIpv", "mpptPow", "mpptUpv", "pow", "uPv",
        ] {
            rem.remove(&format!("{k}{i}"));
            rem.remove(&format!("{k}{i}Str"));
        }
    }

    for secret in ["sn", "sno", "userId"] {
        let _ = rem.remove(secret);
    }

    let mut m = HashMap::with_capacity(100);
    let mut with_units = HashMap::with_capacity(200);
    for (k, _) in rem.clone() {
        if k.ends_with("Str") {
            continue;
        }
        if k.contains("Time") {
            continue;
        }
        let unit = match rem.remove(&format!("{}Str", k)) {
            Some(unit) => unit,
            None => match rem.remove(&format!("{}Unit", k)) {
                Some(unit) => unit,
                None => continue,
            },
        };

        let value = rem.remove(&k).expect("unmodified input list");
        let value = value
            .as_f64()
            .ok_or_else(|| anyhow!("value for {k} not a number: {value:?}"))?;
        let unit = unit.as_str().ok_or_else(|| anyhow!("unit not a string"))?;
        with_units.insert(k, (value, unit.to_string()));
    }

    let periods = ["Total", "Year", "Month", "Yesterday", "Today", ""];

    for class in [
        "backup",
        "gridPurchased",
        "gridSell",
        "homeGrid",
        "homeLoad",
        "generator",
    ] {
        for period in periods {
            let k = format!("{class}{period}Energy");
            let Some((value, unit)) = with_units.remove(&k) else {
                continue;
            };
            let value = to_kwh(value, &unit).with_context(|| anyhow!("processing {k:?}"))?;
            m.insert(
                format!(
                    "energy_{}_{}_kwh",
                    class.to_case(Case::Snake),
                    period.to_ascii_lowercase()
                ),
                value,
            );
        }
    }

    for direction in ["Charge", "Discharge"] {
        for period in periods {
            let k = format!("battery{period}{direction}Energy");
            let Some((value, unit)) = with_units.remove(&k) else {
                continue;
            };
            let value = to_kwh(value, &unit).with_context(|| anyhow!("processing {k:?}"))?;
            m.insert(
                format!(
                    "energy_battery_{}_{}_kwh",
                    direction.to_ascii_lowercase(),
                    period.to_ascii_lowercase()
                ),
                value,
            );
        }
    }

    for (old, new) in [
        ("batteryPower", "battery_power_w"),
        // not sure what any of these are, but they have units
        // export power management?
        ("pEpm", "epm_power_w"),
        ("pEpmSet", "epm_set_power_w"),
        ("bypassLoadPower", "bypass_load_power_w"),
        ("psum", "sum_power_w"),
        ("psumCal", "sum_cal_power_w"),
        ("powTotal", "total_power_w"),
        ("familyLoadPower", "family_load_power_w"),
        ("generatorPower", "generator_power_w"),
        ("totalLoadPower", "total_load_power_w"),
        ("pac", "ac_power_w"),
    ] {
        if let Some((val, unit)) = with_units.remove(old) {
            let val = to_watt(val, &unit).with_context(|| anyhow!("processing {old:?}"))?;
            m.insert(new.to_string(), val);
        }
    }

    if let Some((val, unit)) = with_units.remove("inverterTemperature") {
        ensure!(
            unit.chars().any(|c| matches!(c, 'C' | '℃')),
            "inverter temperature unit not C: {unit:?}"
        );
        m.insert("inverter_temperature_c".to_string(), val);
    }

    if let Some(v) = rem.remove("batteryCapacitySoc") {
        m.insert(
            "battery_soc".to_string(),
            v.as_f64().ok_or_else(|| anyhow!("non-numeric soc"))?,
        );
    }

    println!("{:#?}", with_units);

    // HACK: restoring 'rem', so the legacy support can continue to work
    for (k, (value, unit)) in with_units {
        rem.insert(format!("{k}Str"), json!(unit));
        rem.insert(k, json!(value));
    }

    Ok((m, rem))
}

fn to_kwh(v: f64, unit: &str) -> Result<f64> {
    Ok(match unit {
        "Wh" => v * 0.001,
        "kWh" => v,
        "MWh" => v * 1_000.,
        "GWh" => v * 1_000_000.,
        other => bail!("unknown energy unit: {other:?} for value {v:?}"),
    })
}

fn to_watt(v: f64, unit: &str) -> Result<f64> {
    Ok(match unit {
        "W" => v,
        "kW" => v * 1_000.,
        "MW" => v * 1_000_000.,
        "GW" => v * 1_000_000_000.,
        other => bail!("unknown power unit: {other:?} for value {v:?}"),
    })
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use std::collections::HashMap;

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
    fn test_opinion() -> Result<()> {
        let (good, bad) = super::opinionated(&serde_json::from_str(include_str!(
            "../tests/ref/soliscloud/inverterDetail.json"
        ))?)?;
        let bad = super::map_detail(&bad)?;
        assert_eq!(good.get("energy_home_load_today_kwh"), Some(&6.1));
        println!("{:#?}", bad);
        assert_eq!(bad, HashMap::new());
        assert_eq!(bad.get("family_load_power_kw"), Some(&"0.809".to_string()));
        assert_eq!(bad.get("family_load_power_pec"), Some(&"1".to_string()));
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
