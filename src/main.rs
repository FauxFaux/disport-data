use std::fs;

use anyhow::Result;
use reqwest::Client;
use rusqlite::ToSql;

use crate::config::Config;

mod config;
mod met;
mod owm;
mod soliscloud;

enum Service {
    SolisCloud(soliscloud::Service),
    Met(met::Service),
    Owm(owm::Service),
}

impl Service {
    async fn run(&self, http: &Client) -> Result<()> {
        match self {
            Service::SolisCloud(svc) => soliscloud::run(http, &svc).await,
            Service::Met(svc) => met::run(http, &svc).await,
            Service::Owm(svc) => owm::run(http, &svc).await,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init_timed();

    let solis_config = soliscloud::load_config()?;
    let config: Config = toml::from_str(&fs::read_to_string("config.toml")?)?;

    let http = reqwest::Client::new();
    let archive = rusqlite::Connection::open("archive.db")?;
    migrate(&archive)?;
    let mut svcs = Vec::new();

    svcs.push(Service::SolisCloud(
        soliscloud::warmup(&http, &solis_config).await?,
    ));
    if let Some(met) = config.met {
        svcs.push(Service::Met(met::Service {
            loc: config.loc,
            key: met.key,
        }));
    }
    if let Some(owm) = config.owm {
        svcs.push(Service::Owm(owm::Service {
            loc: config.loc,
            key: owm.key,
        }));
    }

    for svc in &svcs {
        svc.run(&http).await?;
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
