use crate::config::Loc;
use crate::vm::{FullName, Obs};
use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use serde_aux::prelude::*;
use std::ops::Add;
use time::format_description::well_known::Iso8601;
use time::{Duration, OffsetDateTime};

pub struct Service {
    pub loc: Loc,
    pub key: String,
}

pub async fn run(http: &reqwest::Client, svc: &Service) -> Result<Vec<(FullName, Obs)>> {
    let loc = geoutils::Location::new(svc.loc.lat, svc.loc.lon);
    let station = find_nearest(&loc)?;
    let resp: WeatherResponse = http
        .get(format!(
        "http://datapoint.metoffice.gov.uk/public/data/val/wxfcs/all/json/{}?res=3hourly&key={}",
        station.id, svc.key
    ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let forecast = MetForecast::from_response(resp)?;
    println!("{station:?} - {forecast:?}");

    Ok(Vec::new())

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
}

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
    pub data_date: OffsetDateTime,
    pub forecast: Vec<(OffsetDateTime, MetObs)>,
}

impl MetForecast {
    pub fn from_response(resp: WeatherResponse) -> Result<MetForecast> {
        let mut forecast = Vec::new();
        let data_date = OffsetDateTime::parse(&resp.site_rep.dv.data_date, &Iso8601::DEFAULT)?;
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
        Ok(MetForecast {
            data_date,
            forecast,
        })
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
