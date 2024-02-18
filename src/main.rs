use std::fs;

use anyhow::Result;
use reqwest::Client;

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

    let http = Client::new();
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
