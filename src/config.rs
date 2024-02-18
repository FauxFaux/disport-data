use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub loc: Loc,
    pub influx: Influx,
    pub owm: Option<Owm>,
    pub met: Option<Met>,
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
pub struct Influx {
    pub url: String,
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
