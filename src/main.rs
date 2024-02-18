use std::fs;
use std::time::Duration;

use anyhow::Result;
use reqwest::Client;

use crate::config::Config;

mod config;
mod met;
mod owm;
mod soliscloud;
mod vm;

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

    let config = ::config::Config::builder()
        .add_source(::config::File::with_name(".env.toml"))
        .add_source(::config::Environment::with_prefix("DISPORT").separator("_"))
        .build()?;

    let config: Config = serde_path_to_error::deserialize(config)?;

    let http = reqwest::ClientBuilder::default()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(30))
        .build()?;

    let mut svcs = Vec::new();

    if let Some(solis_cloud) = config.solis_cloud {
        svcs.push(Service::SolisCloud(
            soliscloud::warmup(&http, solis_cloud).await?,
        ));
    }
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
