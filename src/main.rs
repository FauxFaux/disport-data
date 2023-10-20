use std::fs;

use anyhow::Result;
use geoutils::Location;
use rusqlite::ToSql;
use serde_json::Value;

use crate::config::Config;
use crate::met::{find_nearest, MetForecast, WeatherResponse};

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
        let station = find_nearest(&loc)?;
        let resp: WeatherResponse = http.get(format!(
            "http://datapoint.metoffice.gov.uk/public/data/val/wxfcs/all/json/{}?res=3hourly&key={}",
            station.id, met.key
        )).send().await?.error_for_status()?.json().await?;
        let forecast = MetForecast::from_response(resp)?;
        println!("{station:?} - {forecast:?}");
    }

    // a period runs from 21:30 yesterday -> 21:30 today
    // chosen due to sunset. or just use actual sunset?
    // probably 21:30 local. When do the met 3h forecasts happen? Want to not line up with those to some extent.
    // otoh, forecast at 9pm isn't particularly relevant; we're focusing on the 6am-6pm period.

    // period: the day we're talking about, either today (before sunset) (0) or tomorrow (1), etc.
    // time: the time on that day (timezone?)
    // source: where the forecast came from, met, owm, etc.
    // advance: how far in advance the forecast was made, 3h, 6h, etc. Round to nearest hour?
    // value: the actual value
    // cloud_cover{period: 0, time: 14:00, source: met, advance: 3h} 77%

    // Can we query this? mean(cloud_cover(period=0, time=14:00, advance: 0h)) is the average of everyone's
    // actual value, where advance:0 means actual?

    // negative advances, does anyone change their actual after the fact?
    // most apis probably just don't have actual

    // Is this a query you can write? `select mean(cloud_cover(period=0, time=14:00, advance: 1-3h))`
    // Is this a query you can write? `graph cloud_cover(period=0, time=14:00, source: met) by advance`

    // in theory you can work out the advance from the observation time in influx. is that easier or harder to query?

    // round all times to the nearest hour?

    // cloud cover is a derived metric, should we be met_cloud_cover, owm_cloud_cover; then the derived value?
    // or are we going to re-derive it from the json if it's boned?

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
