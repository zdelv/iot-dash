use colored::*;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::Row;
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

type Database = HashMap<i32, VecDeque<f32>>;
type SensorIDMap = HashMap<String, i32>;

#[derive(Serialize, Deserialize)]
struct Payload {
    data: f32,
}

enum ChannelMessage {
    Data { data: f32, topic: String },
    Stop,
}

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

async fn get_known_sensors(pool: &PgPool) -> anyhow::Result<HashMap<String, i32>> {
    let rec = sqlx::query("SELECT * FROM sensors")
        .map(|row: PgRow| (row.get("topic"), row.get("sensor_id")))
        .fetch_all(pool)
        .await?;

    let map: HashMap<_, _> = rec.into_iter().collect();
    Ok(map)
}

async fn insert_new_sensor(pool: &PgPool, topic: String) -> anyhow::Result<i32> {
    let rec = sqlx::query(
        r#"
INSERT INTO sensors (topic)
VALUES ($1)
RETURNING sensor_id
        "#,
    )
    .bind(topic)
    .fetch_one(pool)
    .await?;

    Ok(rec.try_get("sensor_id")?)
}

async fn handle_data(
    database: Arc<RwLock<Database>>,
    pool: PgPool,
    mut recv_ch: mpsc::Receiver<ChannelMessage>,
    storage: usize,
) -> anyhow::Result<()> {
    const FLAG_SET_TIME: u64 = 3;

    // Map for topic to sensor_id. IDs are created by the database.
    let mut sensor_id_map: SensorIDMap = get_known_sensors(&pool).await?;
    println!("Found {} sensors in db", sensor_id_map.len());

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

                        let mut db = database.write().await;

                        for (data, topic) in queue.drain(..) {
                            let id: i32 = match sensor_id_map.entry(topic.clone()) {
                                Entry::Occupied(e) => *e.get(),
                                Entry::Vacant(e) => {
                                    let id = insert_new_sensor(&pool, topic.clone()).await?;
                                    e.insert(id);
                                    id
                                }
                            };

                            match db.entry(id) {
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

async fn insert_data(pool: &PgPool, sensor_id: i32, data: &[f32; 3]) -> anyhow::Result<()> {
    let types = vec!["min", "max", "avg"];

    sqlx::query(
        r#"
INSERT INTO stats (sensor_id, type, reading)
VALUES ($1, $2::reading_type[], $3::real[])
        "#,
    )
    .bind(sensor_id)
    .bind(types)
    .bind(data)
    .execute(pool)
    .await?;

    Ok(())
}

async fn parse_data(
    database: Arc<RwLock<Database>>,
    pool: PgPool,
    repeat: u64,
) -> anyhow::Result<()> {
    loop {
        // Only parse data every repeat seconds.
        tokio::time::sleep(Duration::from_secs(repeat)).await;

        {
            // Grab the lock to the database.
            let mut db = database.write().await;

            // Loop through all sensors in the database and compute statistics off
            // of the data. Clear the data vec after using it.
            let mut sensor_count = 0;
            let mut data_count = 0;
            for (sensor_id, data) in db.iter_mut() {
                if !data.is_empty() {
                    let max = data
                        .iter()
                        .max_by(|a, b| a.total_cmp(b))
                        .unwrap_or(&f32::NAN);
                    let min = data
                        .iter()
                        .min_by(|a, b| a.total_cmp(b))
                        .unwrap_or(&f32::NAN);
                    let samples = data.len();
                    let avg: f32 = data.iter().sum::<f32>() / (samples as f32);

                    // Insert data into the remote database.
                    insert_data(&pool, *sensor_id, &[*min, *max, avg]).await?;

                    sensor_count += 1;
                    data_count += data.len();
                    data.clear();
                }
            }
            if sensor_count > 0 {
                println!(
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
    dotenvy::dotenv()?;

    let database_url = match std::env::var("DEV") {
        Ok(_) => std::env::var("DEV_DATABASE_URL")?,
        Err(_) => std::env::var("DATABASE_URL")?
    };

    let mqtt_hostname = match std::env::var("DEV") {
        Ok(_) => std::env::var("DEV_MQTT_HOSTNAME")?,
        Err(_) => std::env::var("MQTT_HOSTNAME")?
    };

    let mqtt_selfname = std::env::var("MQTT_SELFNAME")?;
    let mqtt_port = std::env::var("MQTT_PORT")?.parse::<u16>()?;

    let pool = {
        loop {
            let pool = PgPoolOptions::new()
                .max_connections(3)
                .connect(&database_url)
                .await;

            if let Ok(p) = pool {
                break p
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    };

    let mut mqttoptions = MqttOptions::new(mqtt_selfname, mqtt_hostname, mqtt_port);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    let (client, eventloop) = AsyncClient::new(mqttoptions, 10);
    client.subscribe("home/#", QoS::AtMostOnce).await?;

    // Database of topics mapped to historical values.
    let database: Arc<RwLock<Database>> = Arc::new(RwLock::new(HashMap::new()));
    // Channel used to transfer messages between eventloop and handler
    let (db_write_ch, db_read_ch) = mpsc::channel::<ChannelMessage>(100);

    // MQTT Eventloop
    let eventloop_task = task::spawn(run_eventloop(eventloop, db_write_ch.clone()));
    // Data Handler (pushes data into database)
    let handle_task = task::spawn(handle_data(database.clone(), pool.clone(), db_read_ch, 100));
    // Data Parser (routinely computes statistics on data)
    let parse_task = task::spawn(parse_data(database, pool.clone(), 5));

    // Wait for any of the tasks (or ctrl+c) to return.
    // tokio::task() returns Result<..., JoinError>, meaning if we return a result from the task,
    // then we have nested Results, hence the res? below. If we have a JoinError, just let the
    // program explode, its probably really bad anyways.
    tokio::select! {
        res = eventloop_task => {
            println!("Eventloop Returned");
            if let Err(e) = res? {
                println!("Eventloop Error: {}", e);
            }
        }
        res = handle_task => {
            println!("Handler Returned");
            if let Err(e) = res? {
                println!("Handler Error: {}", e);
            }
        }
        res = parse_task => {
            println!("Parser Returned");
            if let Err(e) = res? {
                println!("Parser Error: {}", e);
            }
        }
        // This currently is canceling the parse_task while there may still be sensor readings in
        // the internal database. This means some readings may be lost when stopping the process.
        // Ensure that no readings are incoming before stopping the process.
        res = tokio::signal::ctrl_c() => {
            if let Err(e) = res {
                println!("Signal Error: {}", e);
            } else {
                println!("Caught ctrl+c, exiting.");
            }
            // Send one last stop message to the handler.
            db_write_ch.send(ChannelMessage::Stop).await?;
        }
    }

    Ok(())
}