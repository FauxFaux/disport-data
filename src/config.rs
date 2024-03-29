use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub loc: Loc,
    pub owm: Option<Owm>,
    pub met: Option<Met>,
    #[serde(rename = "soliscloud")]
    pub solis_cloud: Option<Solis>,
}

#[derive(Copy, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Loc {
    pub lat: f64,
    pub lon: f64,
    // pub dec_deg: u8,
    // pub az_deg: u8,
    // pub kwp: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owm {
    pub key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Met {
    pub key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Solis {
    pub api: String,
    pub key: String,
    pub secret: String,
}
