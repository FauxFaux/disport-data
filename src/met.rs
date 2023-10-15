use crate::met::Weather::HeavyRainShower;
use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use serde_json::Value;

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
struct WeatherResponse {
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
    rep: Vec<Value>,
}

struct MetObs {
    temp_c: f64,
    feels_like_c: f64,
    wind_mph: f64,
    wind_gust_mph: f64,
    wind_dir: char,
    rel_humidity: f64,
    visibility_km: f64,
    precip_prob: f64,
    max_uv: f64,
    weather: Weather,
}

// https://www.metoffice.gov.uk/services/data/datapoint/code-definitions
enum Weather {
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

struct MetForecast {
    forecast: Vec<(time::OffsetDateTime, MetObs)>,
}

impl MetForecast {
    fn from_response(resp: WeatherResponse) -> Result<MetForecast> {
        let mut forecast = Vec::new();
        for period in resp.site_rep.dv.location.period {
            for rep in period.rep {

                let time = time::OffsetDateTime::parse(
                    &format!("{}T{}", period.value, rep.get("T")?.as_str().unwrap()),
                    "%Y-%m-%dT%H:%M:%SZ",
                )?;
                let obs = MetObs {
                    temp_c: rep.get("T")?.as_str().unwrap().parse::<f64>()?,
                    feels_like_c: rep.get("F")?.as_str().unwrap().parse::<f64>()?,
                    wind_mph: rep.get("S")?.as_str().unwrap().parse::<f64>()?,
                    wind_gust_mph: rep.get("G")?.as_str().unwrap().parse::<f64>()?,
                    wind_dir: rep.get("D")?.as_str().unwrap().chars().next().unwrap(),
                    rel_humidity: rep.get("H")?.as_str().unwrap().parse::<f64>()?,
                    visibility_km: rep.get("V")?.as_str().unwrap().parse::<f64>()?,
                    precip_prob: rep.get("Pp")?.as_str().unwrap().parse::<f64>()?,
                    max_uv: rep.get("U")?.as_str().unwrap().parse::<f64>()?,
                    weather: Weather::from_code(rep.get("W")?.as_str().unwrap())?
                        .ok_or(anyhow!("no weather code"))?,
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
        // assert_eq!(resp.site_rep.dv.data_type, "Forecast");
    }
}
