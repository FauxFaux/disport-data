use std::fs;

use anyhow::Result;
use geoutils::Location;
use rusqlite::ToSql;
use serde_json::Value;

use crate::config::Config;
use crate::met::find_nearest;

mod config;
mod met;

#[tokio::main]
async fn main() -> Result<()> {
    let config: Config = toml::from_str(&fs::read_to_string("config.toml")?)?;
    let http = reqwest::Client::new();
    let archive = rusqlite::Connection::open("archive.db")?;
    migrate(&archive)?;

    if let Some(met) = config.met {
        let loc = Location::new(config.loc.lat, config.loc.lon);
        println!("{:?}", find_nearest(&loc)?);
    }

    return Ok(());
    if let Some(owm) = config.owm {
        let resp: Value = http
            .get(format!(
                "https://api.openweathermap.org/data/3.0/onecall?lat={lat}&lon={lon}&appid={key}",
                lat = config.loc.lat,
                lon = config.loc.lon,
                key = owm.key
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        println!("{}", serde_json::to_string_pretty(&resp)?);
    }
    Ok(())
}

fn insert(
    conn: &rusqlite::Connection,
    uri: impl AsRef<str>,
    status: impl Into<u16>,
    res: impl AsRef<str>,
) -> Result<()> {
    let uri = uri.as_ref();
    let text = res.as_ref();
    let status: u16 = status.into();
    conn.execute(
        "INSERT INTO archive (req, status_code, res) VALUES (?, ?, ?)",
        [&uri as &dyn ToSql, &status, &text],
    )?;
    Ok(())
}

fn migrate(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS migrations (
            id INTEGER PRIMARY KEY
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS archive (
            id INTEGER PRIMARY KEY,
            req TEXT NOT NULL,
            status_code INTEGER NOT NULL,
            res TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "INSERT INTO migrations (id) VALUES (1) ON CONFLICT DO NOTHING",
        [],
    )?;
    Ok(())
}
