use anyhow::anyhow;
use anyhow::Context;
use colored::*;
use rumqttc::{Event, EventLoop, Packet};
use serde::{Deserialize, Serialize};
use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

use crate::connection::DbApiConnection;

/// Internal "database" used to store data for each sensor.
/// String maps to a sensor Topic. VecDeque<f32> is a series of data, routinely cleared.
pub type SensorDataMap = HashMap<String, VecDeque<f32>>;
/// A map of sensors to their ids (as assigned by the DB)
type SensorIDMap = HashMap<String, i32>;

/// Deserializable representation of a sensor's configuration. This is expected to be in the config
/// as an item in a list. See config.yaml for more information on how the config should be
/// structured.
#[derive(Deserialize, Debug)]
pub struct SensorConfig {
    #[serde(rename = "name")]
    pub _name: String,
    pub topic: String,
    pub operations: Vec<common::ReadingType>,
}

/// Deserializable representation of the config file. We expect to see this incoming for our
/// config.
#[derive(Deserialize, Debug)]
pub struct Config {
    pub interval: u32,
    pub sensors: Vec<SensorConfig>,
}

/// A very simple wrapping around a regex string that defines a "topic".
///
/// A topic in MQTT is the posting address for a publisher. For example, a temperature sensor might
/// post to a topic of "/home/living_room/temp/1". These are generally handled as strings and can
/// be decomposed based on some MQTT rules (e.g., "/home/*" means all topics under "/home").
///
/// Rather than follow those rules perfectly (we don't need them here; we aren't a broker), we
/// instead just use regex to determine if a topic matches a sensor.
#[derive(Debug)]
pub struct Topic {
    regex: regex::Regex,
}

/// Helper to just clean up code such that a String/&str can be transformed into a Regex.
impl From<&str> for Topic {
    fn from(value: &str) -> Self {
        Topic {
            regex: regex::Regex::new(value).unwrap(),
        }
    }
}

/// Information about each sensor, as described by the config file.
///
/// This is _very_ similar to SensorConfig, but is different in that topic becomes a Topic rather
/// than just a String. We _could_ merge the two of these by building custom Deserialize
/// implementations and Deserializing directly into a Topic object, but that seems like a good
/// place for a future enhancement.
/// TODO: Merge SensorInfo and SensorConfig.
#[derive(Debug)]
pub struct SensorInfo {
    pub topic: Topic,
    pub operations: Vec<common::ReadingType>,
}

impl SensorInfo {
    pub fn new(topic: Topic, operations: Vec<common::ReadingType>) -> Self {
        SensorInfo { topic, operations }
    }
}

/// The expected payload of each sensor.
/// TODO: This should not be a rust type. We should be decoding from raw f32 bits.
#[derive(Serialize, Deserialize, Debug)]
struct Payload {
    data: f32,
}

/// Message used to communicate between the eventloop and handler (handle_data).
#[derive(Debug)]
pub struct ChannelMessage {
    data: f32,
    topic: String
}

/// Main eventloop for MQTT.
///
/// This is separated from handle_data due to the heartbeat Ping requests being time sensitive. We
/// can't miss a heartbeat because we were calculating something or pushing to a queue.
///
/// All data recieved by the eventloop is immediately deserialized and a ChannelMessage is placed
/// into the channel connecting it to the Handler (handle_data).
pub async fn run_eventloop(
    mut eventloop: EventLoop,
    send_ch: mpsc::Sender<ChannelMessage>,
    token: CancellationToken,
    _shutdown_channel: mpsc::Sender<()>,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            _ = token.cancelled() => {
                tracing::info!("Cancelling eventloop!");
                return Ok(());
            }
            not = eventloop.poll() => {
                if let Event::Incoming(Packet::Publish(p)) = not? {
                    let topic = p.topic;
                    let data = bincode::deserialize(&p.payload)?;

                    send_ch.send(ChannelMessage { data, topic }).await?;
                }
            }
        }
    }
}

/// Helper function to push data into the SensorDataMap from the Queue. This is primarly here to
/// reduce code duplication in the handle_data function.
async fn push_data(map: &Arc<RwLock<SensorDataMap>>, queue: &mut Vec<(f32, String)>) {
    let mut db = map.write().await;
    for (data, topic) in queue.drain(..) {
        match db.entry(topic) {
            Entry::Occupied(mut e) => {
                let v = e.get_mut();
                v.push_back(data);
            }
            Entry::Vacant(e) => {
                let mut v = VecDeque::new();
                v.push_back(data);
                e.insert(v);
            }
        }
    }
}

/// Handler task for all data coming in from the MQTT broker.
///
/// The duties of the handler are the following:
///  - Read in incoming data and fill a global queue.
///     - This queue is emptied every FLAG_SET_TIME seconds or when the queue fills to storage
///     elements.
///     - The queue is emptied to the SensorDataMap (a RwLock<HashMap>>) that holds all sensors mapped
///     to their own VecDeque.
pub async fn handle_data(
    sensor_data_map: Arc<RwLock<SensorDataMap>>,
    mut recv_ch: mpsc::Receiver<ChannelMessage>,
    storage: usize,
    token: CancellationToken,
    _shutdown_channel: mpsc::Sender<()>,
) -> anyhow::Result<()> {
    // Time in seconds that we wait until we need to clear out the queue. This gets reset everytime
    // more data gets sent down the channel.
    const CLEAR_TIME: u64 = 3;

    // Rather than push every event into the database by grabbing and releasing the lock over and
    // over, we feed into a queue that stores some number of events. When this queue reaches
    // storage in capacity, we dump the queue to the database.
    let mut queue: Vec<(f32, String)> = Vec::with_capacity(storage);

    loop {
        tokio::select! {
            _ = token.cancelled() => {
                tracing::info!("Cancelling handle_data!");

                // We have to dump the queue if it has any data left in it.
                push_data(&sensor_data_map, &mut queue).await;

                return Ok(());
            }
            _ = tokio::time::sleep(Duration::from_secs(CLEAR_TIME)) => {
                push_data(&sensor_data_map, &mut queue).await;
            }
            Some(ChannelMessage { data, topic }) = recv_ch.recv() => {
                // Place storage amount of information into a queue to prevent us continually
                // locking and unlocking the database. After the queue is filled, we dump the
                // queue to the database.
                queue.push((data, topic));
                if queue.len() == storage {
                    push_data(&sensor_data_map, &mut queue).await;
                }
            }
        }
    }
}

/// All returns from the db should have this structure:
/// {"count": 123, "result": <some_type>}
#[derive(Deserialize)]
pub struct Return<T> {
    #[serde(rename = "count")]
    pub _count: i32,
    pub result: T,
}

/// When recieving sensors from the DB, we expect to recieve the following object:
/// {"count": 123, "result": { "sensors": [{ "topic": "/a/topic", "sensor_id": 123 }, ...] } }
/// This is a Return object, with T: SensorsReturn.
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct SensorsReturn {
    pub sensors: Vec<common::Sensor>,
}

/// Main outside wrapper for a readings post.
///
/// This is used when POSTing some calculated reading to the DB through the db-api.
/// The structure that this matches with is:
/// {"readings": [{"sensor_id": 123, "reading_type": ["average", ...], "reading": [12.0, ...]}, ...] }
#[derive(Serialize, Debug, Default)]
pub struct ReadingsPost {
    pub readings: Vec<ReadingsPostItem>,
}

/// The inner portion of a POST to readings. See ReadingsPost for more information on what the
/// structure of this is.
#[derive(Serialize, Debug, Default)]
pub struct ReadingsPostItem {
    pub sensor_id: i32,
    pub reading_type: Vec<common::ReadingType>,
    pub reading: Vec<f32>,
}

/// The return we expect from the db-api after inserting readings. These are the ids of the
/// readings inserted. We don't do anything with them right now, but they're useful to keep around.
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ReadingsPostReturn {
    pub reading_ids: Vec<i32>,
}

/// Task that parses data after reaching the handler.
/// This is the sole communicator with the db-api.
///
/// Parsing here means the following:
///  - Ensure that the sensor exists in the DB. This is done by pulling the current sensors at
///  task start. At every push to the db-api, the sensors are checked to ensure that they exist in
///  the DB. The sensor is added to the DB if it does not exist in the DB.
///  - Empty the data from the local "database" (a RwLock<HashMap>). This is filled by the handler
///  (handle_data).
///  - Perform "operations" on the incoming readings. Some of the sensors potentially produce
///  massive amounts of data, which is prohibitive to store in a DB without excessive storage
///  capabilties.
///     - The solution in use here to help with this data growth is to perform some statistic
///     across a period of recieved samples. For example, we may calculate the average of a set of
///     samples accumulated over a 5 second interval. Multiple operations may be used
///     simultaneously (e.g., average, minimum, and maximum)..
///     - The outputs of these operations are pushed to the DB instead of the raw data. The
///     operations performed is marked alongside the reading.
pub async fn parse_data(
    sensor_data_map: Arc<RwLock<SensorDataMap>>,
    client: impl DbApiConnection,
    sensor_infos: Vec<SensorInfo>,
    repeat: u64,
    token: CancellationToken,
    _shutdown_channel: mpsc::Sender<()>, // This channel is just used to signal tasks shutdown
) -> anyhow::Result<()> {
    // Map for topic to sensor_id. IDs are created by the database.
    let mut sensor_id_map: SensorIDMap = client.get_all_sensors().await?;
    tracing::info!("Found {} sensors in db", sensor_id_map.len());

    loop {
        tokio::select! {
            _ = token.cancelled() => {
                tracing::info!("Cancelling parse_data!");
                return Ok(());
            }
            // Only parse data every repeat seconds.
            _ = tokio::time::sleep(Duration::from_secs(repeat)) => {
                // Grab the lock to the database.
                let mut smap = sensor_data_map.write().await;

                // Loop through all sensors in the database and compute statistics off
                // of the data. Clear the data vec after using it.
                let mut sensor_count = 0;
                let mut data_count = 0;
                for (topic, data) in smap.iter_mut() {
                    let id = match sensor_id_map.entry(topic.clone()) {
                        Entry::Occupied(e) => *e.get(),
                        Entry::Vacant(e) => {
                            let id = client.insert_sensors(vec![&topic.clone()]).await?;
                            let id = *id.first().context("Failed to get the first id!")?;
                            e.insert(id);
                            id
                        }
                    };

                    // Only do calculations if we have some amount of data for the sensor. We do not
                    // push empty data to the DB to signify "no operations".
                    if !data.is_empty() {
                        for info in &sensor_infos {
                            if info.topic.regex.is_match(topic) {
                                let mut results = Vec::with_capacity(info.operations.len());

                                for op in &info.operations {
                                    results.push(apply_op(op, data));
                                }

                                // Insert data into the remote database.
                                let reading_ids = client
                                    .insert_readings(id, &info.operations, &results)
                                    .await?;

                                if reading_ids.is_empty() {
                                    return Err(anyhow!(
                                        "Did not recieve any reading ids for this submission!"
                                    ));
                                }

                                sensor_count += 1;
                                data_count += data.len();
                            }
                        }

                        data.clear();
                    }
                }
                if sensor_count > 0 {
                    tracing::info!(
                        "Num Sensors: {}, Num Data: {}",
                        sensor_count.to_string().blue(),
                        data_count.to_string().blue()
                    );
                }
            }
        }
    }
}

fn apply_op(op: &common::ReadingType, data: &VecDeque<f32>) -> f32 {
    match op {
        common::ReadingType::Average => {
            let samples = data.len();
            data.iter().sum::<f32>() / (samples as f32)
        }
        common::ReadingType::Maximum => *data
            .iter()
            .max_by(|a, b| a.total_cmp(b))
            .unwrap_or(&f32::NAN),
        common::ReadingType::Minimum => *data
            .iter()
            .min_by(|a, b| a.total_cmp(b))
            .unwrap_or(&f32::NAN),
        common::ReadingType::Count => data.len() as f32,
        common::ReadingType::Median => {
            // Naive approach runs in O(nlogn). Can replace with
            // quickselect for O(n), on average.

            let mut data = data.clone();
            data.make_contiguous().sort_by(|a, b| a.total_cmp(b));

            let num_elems = data.len();
            let midpoint = num_elems / 2;

            // If odd, return midpoint.
            if num_elems % 2 == 1 {
                data[midpoint]
            // If even, return average of two middle values.
            } else {
                0.5 * (data[midpoint - 1] + data[midpoint])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use common::ReadingType;
    use std::time::Duration;
    use tokio::time;

    /// Test that handle_data properly reads from it's recieve channel, clears its queue, and
    /// gracefully exits when told to.
    #[tokio::test]
    async fn test_handle_data() {
        // Empty sensor_data_map
        let sensor_data_map = Arc::new(RwLock::new(SensorDataMap::new()));

        const CHANNEL_SIZE: usize = 5;
        let (send_ch, recv_ch) = tokio::sync::mpsc::channel(CHANNEL_SIZE);

        // Send enough messages to fill the channel.
        // This should force the handler to begin clearing the internal queue.
        for i in 0..CHANNEL_SIZE {
            send_ch
                .send(ChannelMessage {
                    data: i as f32,
                    topic: format!("/home/{}", i),
                })
                .await
                .unwrap();
        }

        // Create our token for cancelling the task and the channel we wait after the task
        // finishes.
        let token = CancellationToken::new();
        let (shutdown_send, mut shutdown_recv) = mpsc::channel(1);

        let h = handle_data(
            sensor_data_map.clone(),
            recv_ch,
            CHANNEL_SIZE,
            token.clone(),
            shutdown_send,
        );

        // Wait 500ms before sending the cancel command to the handler. It should finish by then as
        // it does not wait before reading from the channel and clearing its queue.
        tokio::select! {
            res = h => {
                if let Err(e) = res {
                    println!("handle_data returned an error: {}", e);
                }
            }
            _ = time::sleep(Duration::from_millis(500)) => {
                println!("Starting cancel!");
                // Send the cancellation to the token.
                token.cancel();
                // Wait for the channel to close (ignore the error).
                shutdown_recv.recv().await;
            }
        }

        // Verify that everything went okay.

        // Grab a read handle to the sensor data
        let s = sensor_data_map.read().await;

        println!("{:?}", s);

        // Check that each sensor is properly mapped.
        for i in 0..CHANNEL_SIZE {
            if let Some(c) = s.get(&format!("/home/{}", i)) {
                println!("{}", i);
                assert!(!c.is_empty());
                assert_eq!(c[0], i as f32);
            } else {
                println!("{}", i);
                assert!(false)
            }
        }
    }

    /// Test that the apply_op function correctly calculates statistics across two basic arrays of
    /// data. This tests all operations in common::ReadingType.
    #[test]
    fn test_apply_op() {
        use common::ReadingType::*;

        let data1 = VecDeque::from_iter(vec![1., 2., 3., 4., 5.].into_iter());
        let data2 = VecDeque::from_iter(vec![1., 2., 3., 4.].into_iter());

        assert_eq!(apply_op(&Average, &data1), 3.0);

        assert_eq!(apply_op(&Maximum, &data1), 5.0);
        assert_eq!(apply_op(&Minimum, &data1), 1.0);

        assert_eq!(apply_op(&Count, &data1), 5.0);

        // Median has two code paths depending on the length of the data.
        assert_eq!(apply_op(&Median, &data1), 3.0);
        assert_eq!(apply_op(&Median, &data2), 2.5);
    }

    /// Test that parse_data operates as expected given a mock of the connection to the db-api
    /// service. This uses the DbApiConnection trait to implement a mock with hardcoded responses
    /// to all functions called by parse_data.
    #[tokio::test]
    async fn test_parse_data() {
        // Custom mock used to fake a connection to the db-api. This is only used for implementing
        // DbApiConnection (which the real client, DbApiClient also does).
        struct DbApiClientMock {}

        #[async_trait]
        impl DbApiConnection for DbApiClientMock {
            // Default the connection to true. This isn't used for this test.
            async fn is_connection_up(&self) -> bool {
                true
            }

            // Return a single sensor, acting as if the database has at least one sensor.
            async fn get_all_sensors(&self) -> anyhow::Result<HashMap<String, i32>> {
                let mut map = HashMap::new();

                map.insert("/home/temp1".to_string(), 1);
                Ok(map)
            }

            // Check that the topics vec is both a length of one and the first element is what we
            // expect (see below this trait impl for more info).
            //
            // Return a vec with a single sensor id, with an id one greater than what is already.
            // "in the database" (see get_all_sensors).
            async fn insert_sensors(&self, topics: Vec<&str>) -> anyhow::Result<Vec<i32>> {
                assert_eq!(topics.len(), 1);

                assert_eq!(topics.first(), Some(&"/home/temp2"));

                Ok(vec![2])
            }

            // Check that the sensor_id is 2 (see insert_sensors). Check that all values of
            // operations and data match what we expect.
            //
            // Return a vec with a single reading_id in it. We don't care about the value.
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

                // Check that everything we get is correct.
                assert_eq!(sensor_id, 2);
                for (u, v) in operations
                    .iter()
                    .zip(&vec![ReadingType::Average, ReadingType::Minimum])
                {
                    assert_eq!(u, v);
                }
                for (u, v) in data.iter().zip(&vec![0.0, 1.0, 2.0]) {
                    assert_eq!(u, v);
                }

                // Return
                // We don't know or care about what the reading_id is.
                Ok(vec![123])
            }
        }

        // Create a mock client from the above definition.
        let client = DbApiClientMock {};
        // Create an empty map for sensor data.
        let sensor_data_map = Arc::new(RwLock::new(SensorDataMap::new()));

        // Fill in the map with fake data, simulating if the handle_data function had recieved data
        // from a single sensor for a few readings. The sensor is NOT in the database yet (as of
        // the return from the mock).
        {
            let mut sdm = sensor_data_map.write().await;
            sdm.insert(
                "/home/temp2".to_string(),
                VecDeque::from_iter(vec![0.0, 1.0, 2.0].into_iter()),
            );
        }

        // Create a SensorInfo vec that contains a single SensorInfo for one topic that matches the
        // sensors in the sensor_data_map (and database).
        let sensor_infos: Vec<SensorInfo> = vec![SensorInfo::new(
            Topic::from(r#"/home/\d+"#),
            vec![ReadingType::Average, ReadingType::Minimum],
        )];

        // How long to wait till parse_data reads through the sensor_data_map.
        let repeat = 1;

        let (shutdown_send, mut shutdown_recv) = mpsc::channel(1);
        let token = CancellationToken::new();
        let p = parse_data(
            sensor_data_map.clone(),
            client,
            sensor_infos,
            repeat,
            token.clone(),
            shutdown_send,
        );

        // Allow parse_data to run for at least one interval (repeat seconds).
        // We then send the shutdown signal on the token and wait for the channel to be dropped
        // (The channel dropping means that the task is finished).
        tokio::select! {
            res = p => {
                if let Err(e) = res {
                    println!("Error on return from parse_data: {}", e);
                }
            }
            _ = time::sleep(Duration::from_secs(repeat + 1)) => {
                println!("Starting cancel!");
                // Send the cancellation to the token.
                token.cancel();
                // Wait for the channel to close (ignore the error).
                shutdown_recv.recv().await;
            }
        }

        // The data we push into the map MUST be gone for us to consider this a pass. This ensures
        // that parse_data actually did any work with the mock.
        let sdm = sensor_data_map.write().await;
        assert!(sdm.get(&"/home/temp2".to_string()).unwrap().is_empty())
    }
}
