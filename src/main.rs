use std::env;

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as b64;
use base64::Engine;
use chrono::Utc;
use hmac::Mac;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

type HmacSha1 = hmac::Hmac<sha1::Sha1>;

#[tokio::main]
async fn main() -> Result<()> {
    let req = {
        Req {
            api: env_var("SOLIS_API")?.trim_end_matches('/').to_string(),
            key: env_var("SOLIS_KEY")?,
            secret: env_var("SOLIS_SECRET")?,
        }
    };

    let data = json!({
        "pageNo": 1,
        "pageSize": 10,
    });
    let path = "/v1/api/userStationList";
    let client = reqwest::Client::new();

    let resp = call_api::<Value>(&client, &req, path, &data).await?;
    println!("{}", serde_json::to_string(&resp)?);
    Ok(())
}

struct Req {
    api: String,
    key: String,
    secret: String,
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
