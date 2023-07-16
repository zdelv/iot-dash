use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_with::{formats::Strict, TimestampMilliSeconds};
use sqlx::{FromRow, postgres::{PgHasArrayType, PgTypeInfo}};


#[serde_with::serde_as]
#[derive(Serialize, Deserialize, FromRow)]
pub struct Reading {
    reading_id: i32,
    sensor_id: i32,
    #[serde_as(as = "TimestampMilliSeconds<i64, Strict>")]
    timestamp: DateTime<Utc>,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    reading_type: Vec<ReadingType>,
    reading: Vec<f32>,
}

#[derive(Serialize, Deserialize, Debug, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "reading_type")]
#[sqlx(rename_all = "lowercase")]
pub enum ReadingType {
    Average,
    Median,
    Minimum,
    Maximum,
    Count,
}

// PgHasArrayType is used to give information to sqlx about the type of array that should be used
// to store this value in Postgres. This _should_ be auto implemented onto any type that also
// derives sqlx::Type (while the postgres feature is enabled), but that isn't working for some
// reason, so this is a manual implementation.
impl PgHasArrayType for ReadingType {
    fn array_type_info() -> PgTypeInfo {
        // NOTE: The type name has an underscore prefixed for some reason.
        // I have never seen the db report the type name with the underscore, so I'm not sure where that's
        // coming from...
        PgTypeInfo::with_name("_reading_type")
    }
}

impl std::fmt::Display for ReadingType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let o = match self {
            ReadingType::Average => "average",
            ReadingType::Median => "median",
            ReadingType::Minimum => "minimum",
            ReadingType::Maximum => "maximum",
            ReadingType::Count => "count",
        };
        f.write_str(o)
    }
}

#[derive(Serialize, Deserialize, FromRow)]
pub struct Sensor {
    sensor_id: i32,
    topic: String,
}


#[cfg(test)]
mod tests {
    use crate::Reading;

    #[test]
    fn test_reading_serde() {
        let test = r#"{"reading_id": 10, "sensor_id": 20, "timestamp": 1689434031, "type": ["count", "minimum", "maximum"], "reading": [10.0, 1.5, 20.2]}"#;

        let _reading: Reading = serde_json::from_str(test).unwrap();
    }
}
