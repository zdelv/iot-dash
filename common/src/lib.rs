use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_with::{formats::Strict, TimestampSeconds};
use sqlx::{
    postgres::{PgHasArrayType, PgTypeInfo},
    FromRow,
};


/// A newtype wraper for sensor ids. All sensors have a unique id.
///
/// Currently unused
#[derive(Serialize, Deserialize, Debug, sqlx::Type)]
pub struct SensorId(pub i32);

/// A singular reading. One reading may comprise more than one set of statistics on the raw data.
/// Each statistic is denoted by the reading_type and reading vectors, where corresponding indices
/// relate to each other (e.g., the reading type at idx i corresponds to the reading at idx i).
#[serde_with::serde_as]
#[derive(Serialize, Deserialize, FromRow, Debug)]
pub struct Reading {
    pub reading_id: i32,
    pub sensor_id: i32,
    #[serde_as(as = "TimestampSeconds<i64, Strict>")]
    pub timestamp: DateTime<Utc>,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub reading_type: Vec<ReadingType>,
    pub reading: Vec<f32>,
}

/// The possible reading types. These are statistics computed on the underlying raw data. All of
/// these reduce large amounts of data down to a single value.
#[derive(Serialize, Deserialize, Debug, Clone, sqlx::Type)]
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

/// A sensor. Sensors publish raw data (containing a floating point value) to the Ingest service.
/// This service computes some set of statistics on the raw data to reduce the data size.
///
/// Sensors are designed to be lightweight and have low latency from the time of data acquisition
/// and backend notification.
#[derive(Serialize, Deserialize, sqlx::Type, FromRow, Debug)]
pub struct Sensor {
    pub sensor_id: i32,
    pub topic: String,
}

impl PgHasArrayType for Sensor {
    fn array_type_info() -> PgTypeInfo {
        // NOTE: The type name has an underscore prefixed for some reason.
        // I have never seen the db report the type name with the underscore, so I'm not sure where that's
        // coming from...
        PgTypeInfo::with_name("_sensor")
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, NaiveDateTime, Utc};

    use crate::{Sensor, Reading, ReadingType};

    #[test]
    fn test_reading_serde() {
        let test = r#"{"reading_id":10,"sensor_id":20,"timestamp":1689434031,"type":["count","minimum","maximum"],"reading":[10.0,1.5,20.2]}"#;

        let known_good = Reading {
            reading_id: 10,
            sensor_id: 20,
            timestamp: DateTime::<Utc>::from_utc(
                NaiveDateTime::from_timestamp_opt(1689434031, 0).unwrap(), Utc
            ),
            reading_type: vec![
                ReadingType::Count,
                ReadingType::Minimum,
                ReadingType::Maximum,
            ],
            reading: vec![10.0, 1.5, 20.2],
        };
        let known_good = serde_json::to_string(&known_good).unwrap();

        assert_eq!(test, known_good);
    }

    #[test]
    fn test_sensor_serde() {
        let test = r#"{"sensor_id":123,"topic":"/this/is/a/topic"}"#;

        let known_good = Sensor {
            sensor_id: 123,
            topic: "/this/is/a/topic".to_string()
        };
        let known_good = serde_json::to_string(&known_good).unwrap();

        assert_eq!(test, known_good);
    }
}
