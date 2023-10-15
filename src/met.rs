use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use serde_aux::prelude::*;
use std::ops::Add;
use time::format_description::well_known::Iso8601;
use time::Duration;

#[derive(Deserialize)]
struct Sites {
    #[serde(rename = "Locations")]
    locations: Locations,
}

#[derive(Deserialize)]
struct Locations {
    #[serde(rename = "Location")]
    locations: Vec<Location>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Location {
    latitude: String,
    longitude: String,
    id: String,
    name: String,
    elevation: Option<String>,
    region: Option<String>,
    unitary_auth_area: Option<String>,
    obs_source: Option<String>,
    national_park: Option<String>,
}

#[derive(Debug)]
pub struct MetLocation {
    pub id: String,
    pub name: String,
    pub location: geoutils::Location,
    pub elevation: Option<f64>,
    pub region: Option<String>,
    pub unitary_auth_area: Option<String>,
    pub obs_source: Option<String>,
    pub national_park: Option<String>,
}

pub fn find_nearest(target: &geoutils::Location) -> Result<MetLocation> {
    // curl 'http://datapoint.metoffice.gov.uk/public/data/val/wxfcs/all/json/sitelist?key='$KEY > data/met-sites.json
    let mut v: Sites = serde_json::from_str(include_str!("../data/met-sites.json"))?;
    let mut closest = v
        .locations
        .locations
        .pop()
        .ok_or(anyhow!("no locations in asset"))?;
    let mut closest_dist = f64::MAX;
    for cand in v.locations.locations {
        let lat = cand.latitude.parse::<f64>()?;
        let lon = cand.longitude.parse::<f64>()?;
        let loc = geoutils::Location::new(lat, lon);
        let dist = loc
            .distance_to(&target)
            .map_err(|e| anyhow!("{e}"))?
            .meters();
        if dist < closest_dist {
            closest = cand;
            closest_dist = dist;
        }
    }
    let elevation = match closest.elevation {
        Some(e) => Some(e.parse::<f64>()?),
        None => None,
    };

    Ok(MetLocation {
        id: closest.id,
        name: closest.name,
        location: geoutils::Location::new(
            closest.latitude.parse::<f64>()?,
            closest.longitude.parse::<f64>()?,
        ),
        elevation,
        region: closest.region,
        unitary_auth_area: closest.unitary_auth_area,
        obs_source: closest.obs_source,
        national_park: closest.national_park,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WeatherResponse {
    site_rep: SiteRep,
}

#[derive(Deserialize)]
struct SiteRep {
    #[serde(rename = "Wx")]
    wx: Wx,
    #[serde(rename = "DV")]
    dv: Dv,
}

#[derive(Deserialize)]
struct Wx {
    #[serde(rename = "Param")]
    param: Vec<Param>,
}

#[derive(Deserialize)]
struct Param {
    name: String,
    units: String,
    // "$" means "text in the xml document", but this is json
    #[serde(rename = "$")]
    desc: String,
}

#[derive(Deserialize)]
struct Dv {
    #[serde(rename = "dataDate")]
    data_date: String,
    #[serde(rename = "type")]
    data_type: String,
    #[serde(rename = "Location")]
    location: ForecastLocation,
}

#[derive(Deserialize)]
struct ForecastLocation {
    // id fields from the site list omitted
    #[serde(rename = "Period")]
    period: Vec<Period>,
}

#[derive(Deserialize)]
struct Period {
    #[serde(rename = "type")]
    period_type: String,
    #[serde(rename = "value")]
    value: String,
    #[serde(rename = "Rep")]
    rep: Vec<MetRep>,
}

#[derive(Deserialize)]
struct MetRep {
    #[serde(rename = "T", deserialize_with = "deserialize_number_from_string")]
    temp_c: f64,
    #[serde(rename = "F", deserialize_with = "deserialize_number_from_string")]
    feels_like_c: f64,
    #[serde(rename = "S", deserialize_with = "deserialize_number_from_string")]
    wind_mph: f64,
    #[serde(rename = "G", deserialize_with = "deserialize_number_from_string")]
    wind_gust_mph: f64,
    #[serde(rename = "D", deserialize_with = "deserialize_number_from_string")]
    wind_dir: String,
    #[serde(rename = "H", deserialize_with = "deserialize_number_from_string")]
    rel_humidity: f64,
    #[serde(rename = "V")]
    visibility: String,
    #[serde(rename = "Pp", deserialize_with = "deserialize_number_from_string")]
    precip_prob: f64,
    #[serde(rename = "U", deserialize_with = "deserialize_number_from_string")]
    max_uv: f64,
    #[serde(rename = "W")]
    weather: String,

    #[serde(rename = "$", deserialize_with = "deserialize_number_from_string")]
    mins: u32,
}

#[derive(Debug)]
pub struct MetObs {
    pub temp_c: f64,
    pub feels_like_c: f64,
    pub wind_mph: f64,
    pub wind_gust_mph: f64,
    pub wind_dir: String,
    pub rel_humidity: f64,
    pub visibility_km: f64,
    pub precip_prob: f64,
    pub max_uv: f64,
    pub weather: Option<Weather>,
}

// https://www.metoffice.gov.uk/services/data/datapoint/code-definitions
#[derive(Debug)]
pub enum Weather {
    Clear,
    PartlyCloudy,
    Mist,
    Fog,
    Cloudy,
    Overcast,
    TraceRain,
    LightRainShower,
    LightRain,
    Drizzle,
    HeavyRainShower,
    HeavyRain,
    SleetShower,
    Sleet,
    HailShower,
    Hail,
    LightSnowShower,
    LightSnow,
    HeavySnowShower,
    HeavySnow,
    ThunderShower,
    Thunder,
}

impl Weather {
    fn from_code(code: &str) -> Result<Option<Weather>> {
        if code == "NA" {
            return Ok(None);
        }
        use Weather::*;
        Ok(Some(match code.parse::<i8>()? {
            -1 => TraceRain,
            0 | 1 => Clear,
            2 | 3 => PartlyCloudy,
            5 => Mist,
            6 => Fog,
            7 => Cloudy,
            8 => Overcast,
            9 | 10 => LightRainShower,
            11 => Drizzle,
            12 => LightRain,
            13 | 14 => HeavyRainShower,
            15 => HeavyRain,
            16 | 17 => SleetShower,
            18 => Sleet,
            19 | 20 => HailShower,
            21 => Hail,
            22 | 23 => LightSnowShower,
            24 => LightSnow,
            25 | 26 => HeavySnowShower,
            27 => HeavySnow,
            28 | 29 => ThunderShower,
            30 => Thunder,

            _ => bail!("unknown weather code {}", code),
        }))
    }
}

#[derive(Debug)]
pub struct MetForecast {
    forecast: Vec<(time::OffsetDateTime, MetObs)>,
}

impl MetForecast {
    pub fn from_response(resp: WeatherResponse) -> Result<MetForecast> {
        let mut forecast = Vec::new();
        for period in resp.site_rep.dv.location.period {
            for rep in period.rep {
                let value = period
                    .value
                    .strip_suffix("Z")
                    .ok_or(anyhow!("bad period value"))?;
                let time = time::Date::parse(&value, &Iso8601::DEFAULT)?
                    .midnight()
                    .assume_utc()
                    .add(Duration::minutes(i64::from(rep.mins)));

                let obs = MetObs {
                    temp_c: rep.temp_c,
                    feels_like_c: rep.feels_like_c,
                    wind_mph: rep.wind_mph,
                    wind_gust_mph: rep.wind_gust_mph,
                    wind_dir: rep.wind_dir.to_string(),
                    rel_humidity: rep.rel_humidity,
                    visibility_km: match rep.visibility.as_str() {
                        "UN" => f64::NAN,
                        "VP" => 0.5,
                        "PO" => 2.,
                        "MO" => 6.,
                        "GO" => 15.,
                        "VG" => 30.,
                        "EX" => 60.,
                        other => bail!("unknown visibility code {}", other),
                    },
                    precip_prob: rep.precip_prob,
                    max_uv: rep.max_uv,
                    weather: Weather::from_code(&rep.weather)?,
                };
                forecast.push((time, obs));
            }
        }
        Ok(MetForecast { forecast })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sites() {
        let target = geoutils::Location::new(51.5074, 0.1278);
        let found = find_nearest(&target).unwrap();
        assert_eq!(found.name, "Dagenham");
    }

    #[test]
    fn test_response() {
        let resp: WeatherResponse =
            serde_json::from_str(include_str!("../tests/ref/met-folkes.json")).unwrap();
        let forecast = MetForecast::from_response(resp).unwrap();
        assert_eq!(forecast.forecast[0].0.unix_timestamp(), 1696604400);
    }
}
