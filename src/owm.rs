use anyhow::Result;
use reqwest::Client;
use serde_json::Value;

use crate::config::Loc;
use crate::vm::{FullName, Obs};

pub struct Service {
    pub loc: Loc,
    pub key: String,
}

pub async fn run(http: &Client, svc: &Service) -> Result<Vec<(FullName, Obs)>> {
    let resp: Value = http
        .get(format!(
            "https://api.openweathermap.org/data/3.0/onecall?lat={lat}&lon={lon}&appid={key}",
            lat = svc.loc.lat,
            lon = svc.loc.lon,
            key = svc.key
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    println!("{}", serde_json::to_string_pretty(&resp)?);
    Ok(Vec::new())
}
