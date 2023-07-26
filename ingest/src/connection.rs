use anyhow::{anyhow, Context};
use async_trait::async_trait;
use std::collections::HashMap;

use crate::actors::{ReadingsPost, ReadingsPostItem, ReadingsPostReturn, Return, SensorsReturn};

/// A trait representing the connection to the db-api service.
///
/// This exists to help abstract out reqwest for testing purposes. We would like to test parts of
/// this code without an HTTP test server (and preferrably without an HTTP connection at all), so
/// this trait allows for us to design a mock client that is injected where needed.
#[async_trait]
pub trait DbApiConnection {
    /// Makes a request that checks the connection of the db-api.
    async fn is_connection_up(&self) -> bool;

    /// Inserts new sensors into the DB through the db-api.
    async fn insert_sensors(&self, topics: Vec<&str>) -> anyhow::Result<Vec<i32>>;

    /// Finds all sensors in the database by requesting from the db-api.
    async fn get_all_sensors(&self) -> anyhow::Result<HashMap<String, i32>>;

    /// Inserts new readings into the databse through the db-api.
    async fn insert_readings(
        &self,
        sensor_id: i32,
        operations: &[common::ReadingType],
        data: &[f32],
    ) -> anyhow::Result<Vec<i32>>;
}

/// A client used to communicate with the db-api service.
///
/// This is designed to be an impl of the DbApiConnection trait.
pub struct DbApiClient {
    client: reqwest::Client,
    hostname: String,
}

impl DbApiClient {
    pub fn new(client: reqwest::Client, hostname: &str) -> Self {
        DbApiClient { client, hostname: hostname.to_string() }
    }
}

#[async_trait]
impl DbApiConnection for DbApiClient {
    async fn is_connection_up(&self) -> bool {
        self.client.get(&self.hostname).send().await.is_ok()
    }

    async fn insert_sensors(&self, topics: Vec<&str>) -> anyhow::Result<Vec<i32>> {
        let mut sensor = HashMap::new();
        sensor.insert("topics", topics);

        let mut result = self
            .client
            .post(format!("{}/sensor", self.hostname))
            .json(&sensor)
            .send()
            .await?
            .json::<Return<HashMap<String, Vec<i32>>>>()
            .await?;

        let id = result
            .result
            .remove("sensor_ids")
            .context("Could not find the sensor_id field!")?;

        Ok(id)
    }

    async fn get_all_sensors(&self) -> anyhow::Result<HashMap<String, i32>> {
        let ret = self
            .client
            .get(format!("{}/sensor?gt=0", self.hostname))
            .send()
            .await?
            .json::<Return<SensorsReturn>>()
            .await?;

        let mut map = HashMap::new();
        for r in ret.result.sensors.into_iter() {
            map.insert(r.topic.clone(), r.sensor_id);
        }

        Ok(map)
    }

    async fn insert_readings(
        &self,
        sensor_id: i32,
        operations: &[common::ReadingType],
        data: &[f32],
    ) -> anyhow::Result<Vec<i32>> {
        if operations.len() != data.len() {
            return Err(anyhow!(
                "The number of operations does not match the number of data points!"
            ));
        }

        let message = ReadingsPost {
            readings: vec![ReadingsPostItem {
                sensor_id,
                reading_type: operations.to_vec(),
                reading: data.to_vec(),
            }],
        };

        let result = self
            .client
            .post(format!("{}/reading", self.hostname))
            .json(&message)
            .send()
            .await?
            .json::<Return<ReadingsPostReturn>>()
            .await?;

        Ok(result.result.reading_ids)
    }
}
