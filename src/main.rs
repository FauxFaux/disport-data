use std::env;

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as b64;
use base64::Engine;
use chrono::Utc;
use hmac::Mac;
use serde_json::{json, Value};

type HmacSha1 = hmac::Hmac<sha1::Sha1>;

#[tokio::main]
async fn main() -> Result<()> {
    let api = env_var("SOLIS_API")?;
    let api = api.trim_end_matches('/');
    let key = env_var("SOLIS_KEY")?;
    let secret = env_var("SOLIS_SECRET")?;
    let data = json!({
        "pageNo": 1,
        "pageSize": 10,
    });
    let data = serde_json::to_vec(&data)?;
    let md5 = b64.encode(md5::compute(&data).0);
    // TODO: +0000 instead of 'GMT'
    let now = Utc::now().to_rfc2822();
    let path = "/v1/api/userStationList";
    let param = format!("POST\n{md5}\napplication/json\n{now}\n{path}");
    let mut mac = HmacSha1::new_from_slice(secret.as_bytes())?;
    mac.update(param.as_bytes());
    let signature = b64.encode(mac.finalize().into_bytes());
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{api}{path}"))
        .header("Content-Type", "application/json;charset=utf-8")
        .header("Date", now)
        .header("Authorization", format!("API {key}:{signature}"))
        .header("Content-MD5", md5)
        .body(data)
        .send()
        .await?
        .error_for_status()?;

    let resp = resp.json::<Value>().await?;
    println!("{}", serde_json::to_string(&resp)?);
    Ok(())
}

fn env_var(name: &'static str) -> Result<String> {
    env::var(name).with_context(|| anyhow!("reading env var {name:?}"))
}
