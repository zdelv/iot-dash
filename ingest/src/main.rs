use anyhow::{anyhow, Context};
use colored::*;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use serde::{Deserialize, Serialize};
use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    sync::{mpsc, RwLock},
    task,
};
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

/// Internal "database" used to store data for each sensor.
/// String maps to a sensor Topic. VecDeque<f32> is a series of data, routinely cleared.
type SensorDataMap = HashMap<String, VecDeque<f32>>;
/// A map of sensors to their ids (as assigned by the DB)
type SensorIDMap = HashMap<String, i32>;

/// Deserializable representation of a sensor's configuration. This is expected to be in the config
/// as an item in a list. See config.yaml for more information on how the config should be
/// structured.
#[derive(Deserialize, Debug)]
struct SensorConfig {
    #[serde(rename = "name")]
    _name: String,
    topic: String,
    operations: Vec<common::ReadingType>,
}

/// Deserializable representation of the config file. We expect to see this incoming for our
/// config.
#[derive(Deserialize, Debug)]
struct Config {
    interval: u32,
    sensors: Vec<SensorConfig>,
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
struct Topic {
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
struct SensorInfo {
    topic: Topic,
    operations: Vec<common::ReadingType>,
}

impl SensorInfo {
    fn new(topic: Topic, operations: Vec<common::ReadingType>) -> Self {
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
enum ChannelMessage {
    Data { data: f32, topic: String },
    Stop,
}

/// Main eventloop for MQTT.
///
/// This is separated from handle_data due to the heartbeat Ping requests being time sensitive. We
/// can't miss a heartbeat because we were calculating something or pushing to a queue.
///
/// All data recieved by the eventloop is immediately deserialized and a ChannelMessage is placed
/// into the channel connecting it to the Handler (handle_data).
async fn run_eventloop(
    mut eventloop: EventLoop,
    send_ch: mpsc::Sender<ChannelMessage>,
) -> anyhow::Result<()> {
    loop {
        let notification = eventloop.poll().await?;

        if let Event::Incoming(Packet::Publish(p)) = notification {
            let topic = p.topic;
            let data = bincode::deserialize(&p.payload)?;

            send_ch.send(ChannelMessage::Data { data, topic }).await?;
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
async fn handle_data(
    sensor_data_map: Arc<RwLock<SensorDataMap>>,
    mut recv_ch: mpsc::Receiver<ChannelMessage>,
    storage: usize,
) -> anyhow::Result<()> {
    // Interval at which we attempt to force empty the queue, in seconds.
    const FLAG_SET_TIME: u64 = 3;

    // Rather than push every event into the database by grabbing and releasing the lock over and
    // over, we feed into a queue that stores some number of events. When this queue reaches
    // storage in capacity, we dump the queue to the database.
    let mut queue: Vec<(f32, String)> = Vec::with_capacity(storage);

    // Flag used to clear the queue routinely. This is mainly only used when the input sensors stop
    // feeding new events and the queue is partially filled. The flag is set true every
    // FLAG_SET_TIME seconds and forces the queue to be cleared.
    let flag = Arc::new(AtomicBool::new(false));

    // Task to set the flag every FLAG_SET_TIME seconds.
    let flag_clone = flag.clone();
    task::spawn(async move {
        tokio::time::sleep(Duration::from_secs(FLAG_SET_TIME)).await;
        flag_clone.store(true, Ordering::SeqCst);
    });

    loop {
        let data = recv_ch.recv().await;

        if let Some(d) = data {
            match d {
                ChannelMessage::Stop => {
                    return Ok(());
                }
                ChannelMessage::Data { data, topic } => {
                    // Place storage amount of information into a queue to prevent us continually
                    // locking and unlocking the database. After the queue is filled, we dump the
                    // queue to the database.
                    let flag_state = flag.load(Ordering::SeqCst);
                    if flag_state || queue.len() == storage {
                        flag.store(false, Ordering::SeqCst);

                        let mut db = sensor_data_map.write().await;

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
                    } else {
                        queue.push((data, topic));
                    }
                }
            }
        }
    }
}

/// All returns from the db should have this structure:
/// {"count": 123, "result": <some_type>}
#[derive(Deserialize)]
struct Return<T> {
    #[serde(rename = "count")]
    _count: i32,
    result: T,
}

/// When recieving sensors from the DB, we expect to recieve the following object:
/// {"count": 123, "result": { "sensors": [{ "topic": "/a/topic", "sensor_id": 123 }, ...] } }
/// This is a Return object, with T: SensorsReturn.
#[derive(Serialize, Deserialize, Debug, Default)]
struct SensorsReturn {
    sensors: Vec<common::Sensor>,
}

/// Finds all sensors in the database by requesting from the db-api.
async fn get_known_sensors(
    client: &reqwest::Client,
    hostname: &str,
) -> anyhow::Result<SensorIDMap> {
    let ret = client
        .get(format!("{}/sensor?gt=0", hostname))
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

/// Inserts a new sensor into the DB through the db-api.
///
/// Uses a HashMap instead of a custom type for the JSON to save some space. The type would only
/// ever be used here, which is a bit of a waste.
async fn insert_new_sensor(
    client: &reqwest::Client,
    hostname: &str,
    topic: String,
) -> anyhow::Result<i32> {
    let mut sensor = HashMap::new();
    sensor.insert("topics", vec![topic]);

    let result = client
        .post(format!("{}/sensor", hostname))
        .json(&sensor)
        .send()
        .await?
        .json::<Return<HashMap<String, Vec<i32>>>>()
        .await?;

    let id = result
        .result
        .get("sensor_ids")
        .ok_or(anyhow!("Could not find sensor_ids field!"))?
        .first()
        .ok_or(anyhow!("No sensor_ids found!"))?;
    Ok(*id)
}

/// Main outside wrapper for a readings post.
///
/// This is used when POSTing some calculated reading to the DB through the db-api.
/// The structure that this matches with is:
/// {"readings": [{"sensor_id": 123, "reading_type": ["average", ...], "reading": [12.0, ...]}, ...] }
#[derive(Serialize, Debug, Default)]
struct ReadingsPost {
    readings: Vec<ReadingsPostItem>,
}

/// The inner portion of a POST to readings. See ReadingsPost for more information on what the
/// structure of this is.
#[derive(Serialize, Debug, Default)]
struct ReadingsPostItem {
    sensor_id: i32,
    reading_type: Vec<common::ReadingType>,
    reading: Vec<f32>,
}

/// The return we expect from the db-api after inserting readings. These are the ids of the
/// readings inserted. We don't do anything with them right now, but they're useful to keep around.
#[derive(Serialize, Deserialize, Debug, Default)]
struct ReadingsPostReturn {
    reading_ids: Vec<i32>,
}

/// Inserts data into the DB using a POST to the db-api. See ReadingPost for more information on
/// the expected JSON structure to be sent over the POST message.
async fn insert_data(
    client: &reqwest::Client,
    hostname: &str,
    sensor_id: i32,
    operations: &[common::ReadingType],
    data: &[f32],
) -> anyhow::Result<()> {
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

    let _result = client
        .post(format!("{}/reading", hostname))
        .json(&message)
        .send()
        .await?
        .json::<Return<ReadingsPostReturn>>()
        .await?;

    Ok(())
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
async fn parse_data(
    sensor_data_map: Arc<RwLock<SensorDataMap>>,
    client: reqwest::Client,
    hostname: String,
    sensor_infos: Vec<SensorInfo>,
    repeat: u64,
) -> anyhow::Result<()> {
    // Map for topic to sensor_id. IDs are created by the database.
    let mut sensor_id_map: SensorIDMap = get_known_sensors(&client, &hostname).await?;
    tracing::info!("Found {} sensors in db", sensor_id_map.len());

    loop {
        // Only parse data every repeat seconds.
        tokio::time::sleep(Duration::from_secs(repeat)).await;

        {
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
                        let id = insert_new_sensor(&client, &hostname, topic.clone()).await?;
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
                                match op {
                                    common::ReadingType::Average => {
                                        let samples = data.len();
                                        let avg: f32 = data.iter().sum::<f32>() / (samples as f32);
                                        results.push(avg);
                                    }
                                    common::ReadingType::Maximum => {
                                        let max = data
                                            .iter()
                                            .max_by(|a, b| a.total_cmp(b))
                                            .unwrap_or(&f32::NAN);
                                        results.push(*max);
                                    }
                                    common::ReadingType::Minimum => {
                                        let min = data
                                            .iter()
                                            .min_by(|a, b| a.total_cmp(b))
                                            .unwrap_or(&f32::NAN);
                                        results.push(*min);
                                    }
                                    common::ReadingType::Count => {
                                        results.push(data.len() as f32);
                                    }
                                    common::ReadingType::Median => {
                                        // Naive approach runs in O(nlogn). Can replace with
                                        // quickselect for O(n), on average.
                                        data.make_contiguous().sort_by(|a, b| a.total_cmp(b));

                                        let num_elems = results.len();
                                        let midpoint = num_elems / 2;

                                        // If odd, return midpoint.
                                        let med = if num_elems % 2 == 1 {
                                            data[midpoint]
                                        // If even, return average of two middle values.
                                        } else {
                                            0.5 * (data[midpoint - 1] + data[midpoint])
                                        };
                                        results.push(med)
                                    }
                                }
                            }

                            // Insert data into the remote database.
                            insert_data(&client, &hostname, id, &info.operations, &results).await?;
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

#[tokio::main()]
async fn main() -> anyhow::Result<()> {
    // Modify environmental variables by reading in from .env file.
    dotenvy::dotenv()?;

    // Log filter follows env-filter formats. See tracing_subscriber for more info.
    let log_filter = std::env::var("LOG_FILTER").unwrap_or("ingest=info".to_string());
    let log_dir = std::env::var("LOG_DIR").unwrap_or("./".to_string());

    // Create a file appender that automatically outputs all logs to a file, with hourly rollover.
    let file_appender = tracing_appender::rolling::hourly(log_dir, "ingest.log");
    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);

    // Setup the logger with log_filer and the file_writer. tracing_subscriber::fmt::Subscriber
    // comes with a stdout Layer by default. We add the file_writer layer ourselves.
    tracing::subscriber::set_global_default(
        fmt::Subscriber::builder()
            .with_env_filter(log_filter)
            .finish()
            .with(fmt::Layer::default().with_writer(file_writer))
    )?;

    // TODO: Actually handle the errors below. ? should really only be used in functions that call
    // from main. Main should handle all errors and display more useful context or attempt to
    // recover.

    // Read in config file.
    let config = std::env::var("CONFIG")
        .context("Failed to read config file. Please specify using the env var CONFIG.")?;
    let config = std::fs::File::open(config).context("Failed to open config file.")?;
    let config: Config = serde_yaml::from_reader(config).context("Failed to parse config file.")?;

    let interval = config.interval;
    let sensor_infos: Vec<SensorInfo> = config
        .sensors
        .into_iter()
        .map(|s| SensorInfo::new(s.topic.as_str().into(), s.operations))
        .collect();

    let db_api_hostname = match std::env::var("POD") {
        Ok(_) => std::env::var("POD_DB_HOSTNAME")?,
        Err(_) => std::env::var("DB_HOSTNAME")?,
    };

    // Read in hostname to the local MQTT broker.
    let mqtt_hostname = match std::env::var("POD") {
        Ok(_) => std::env::var("POD_MQTT_HOSTNAME")?,
        Err(_) => std::env::var("MQTT_HOSTNAME")?,
    };
    let mqtt_selfname = std::env::var("MQTT_SELFNAME")?;
    let mqtt_port = std::env::var("MQTT_PORT")?.parse::<u16>()?;

    let mut mqttoptions = MqttOptions::new(mqtt_selfname, mqtt_hostname, mqtt_port);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    let (mqtt_client, eventloop) = AsyncClient::new(mqttoptions, 10);
    mqtt_client.subscribe("home/#", QoS::AtMostOnce).await?;

    // Client for communicating with db-api over HTTP
    let http_client = reqwest::Client::new();

    // Attempt to connect to the db-api before continuing.
    loop {
        tracing::info!("Attempting to conenct to db-api: {}", &db_api_hostname);

        if http_client.get(&db_api_hostname).send().await.is_ok() {
            break;
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    // Map of topics mapped to historical values.
    let database: Arc<RwLock<SensorDataMap>> = Arc::new(RwLock::new(HashMap::new()));
    // Channel used to transfer messages between eventloop and handler
    let (db_write_ch, db_read_ch) = mpsc::channel::<ChannelMessage>(100);

    // MQTT Eventloop
    let eventloop_task = task::spawn(run_eventloop(eventloop, db_write_ch.clone()));
    // Data Handler (pushes data into database)
    let handle_task = task::spawn(handle_data(database.clone(), db_read_ch, 100));
    // Data Parser (routinely computes statistics on data)
    let parse_task = task::spawn(parse_data(
        database,
        http_client,
        db_api_hostname,
        sensor_infos,
        interval as u64,
    ));

    // Wait for any of the tasks (or ctrl+c) to return.
    // tokio::task() returns Result<..., JoinError>, meaning if we return a result from the task,
    // then we have nested Results, hence the res? below. If we have a JoinError, just let the
    // program explode, its probably really bad anyways.
    tokio::select! {
        res = eventloop_task => {
            tracing::info!("Eventloop Returned");
            if let Err(e) = res? {
                tracing::error!("Eventloop Error: {}", e);
            }
        }
        res = handle_task => {
            tracing::info!("Handler Returned");
            if let Err(e) = res? {
                tracing::error!("Handler Error: {}", e);
            }
        }
        res = parse_task => {
            tracing::info!("Parser Returned");
            if let Err(e) = res? {
                tracing::error!("Parser Error: {}", e);
            }
        }
        // This currently is canceling the parse_task while there may still be sensor readings in
        // the internal database. This means some readings may be lost when stopping the process.
        // Ensure that no readings are incoming before stopping the process.
        res = tokio::signal::ctrl_c() => {
            if let Err(e) = res {
                tracing::error!("Signal Error: {}", e);
            } else {
                tracing::error!("Caught ctrl+c, exiting.");
            }
            // Send one last stop message to the handler.
            db_write_ch.send(ChannelMessage::Stop).await?;
        }
    }

    Ok(())
}
