use std::collections::HashMap;
use std::env;
use std::time::Duration;

use anyhow::{anyhow, Context, Error, Result};
use base64::engine::general_purpose::STANDARD as b64;
use base64::Engine;
use chrono::Utc;
use hmac::Mac;
use log::error;
use mqtt_reeze::{Mqtt, QoS, Topic};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

type HmacSha1 = hmac::Hmac<sha1::Sha1>;

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init_timed();

    let req = {
        Req {
            api: env_var("SOLIS_API")?.trim_end_matches('/').to_string(),
            key: env_var("SOLIS_KEY")?,
            secret: env_var("SOLIS_SECRET")?,
        }
    };

    let mqtt = Mqtt::new_from_env("soliscloud-mqtt").context("creating mqtt client")?;

    let client = reqwest::Client::new();

    let resp = call_api::<Resp<AllInverters>>(
        &client,
        &req,
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

    let err: Error = 'app: loop {
        for id in &inverter_ids {
            if let Err(err) = publish_one(&client, &req, &mqtt, id).await {
                break 'app err;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    };

    error!("Something failed, trying to shutdown: {:?}", err);

    mqtt.finish().await?;

    Err(err).context("running app")
}

async fn publish_one(client: &Client, req: &Req, mqtt: &Mqtt, id: &str) -> Result<()> {
    let resp = call_api::<Resp<HashMap<String, Value>>>(
        &client,
        &req,
        "/v1/api/inverterDetail",
        &json!({
            "id": id,
        }),
    )
    .await?;
    mqtt.publish_json(
        &Topic::new(
            format!("soliscloud/inverter/{}/detail", id),
            QoS::AtLeastOnce,
            true,
        ),
        &resp.data,
    )
    .await?;
    Ok(())
}

struct Req {
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
