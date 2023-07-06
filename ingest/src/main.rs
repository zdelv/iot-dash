use colored::*;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use serde::{Deserialize, Serialize};
use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    error::Error,
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{mpsc, RwLock},
    task,
};

type Database = HashMap<String, VecDeque<f32>>;

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
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        let notification = eventloop.poll().await?;

        if let Event::Incoming(Packet::Publish(p)) = notification {
            let topic = p.topic;
            let data = bincode::deserialize(&p.payload)?;

            send_ch.send(ChannelMessage::Data { data, topic }).await?;
        }
    }
}

async fn handle_data(
    database: Arc<RwLock<Database>>,
    mut recv_ch: mpsc::Receiver<ChannelMessage>,
    storage: usize
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut queue: Vec<(f32, String)> = Vec::with_capacity(storage);
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
                    if queue.len() == storage {
                        let mut db = database.write().await;

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

async fn parse_data(
    database: Arc<RwLock<Database>>,
    repeat: u64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        // Only parse data every repeat seconds.
        tokio::time::sleep(Duration::from_secs(repeat)).await;

        {
            // Grab the lock to the database.
            let mut db = database.write().await;

            // Loop through all sensors in the database and compute statistics off
            // of the data. Clear the data vec after using it.
            let mut count = 0;
            for (sensor, data) in db.iter_mut() {
                if data.len() > 0 {
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
                    println!("{}: {},{},{},{}", sensor.blue(), samples, min, max, avg);

                    count += 1;
                    data.clear();
                }
            }
            println!("Num Sensors: {}", count);
        }
    }
}

#[tokio::main()]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut mqttoptions = MqttOptions::new("ingest", "0.0", 1883);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    let (client, eventloop) = AsyncClient::new(mqttoptions, 10);
    client.subscribe("home/#", QoS::AtMostOnce).await?;

    // Database of topics mapped to historical values.
    let database: Arc<RwLock<Database>> = Arc::new(RwLock::new(HashMap::new()));
    let (db_write_ch, db_read_ch) = mpsc::channel::<ChannelMessage>(100);

    // MQTT Eventloop
    let eventloop_task = task::spawn(run_eventloop(eventloop, db_write_ch.clone()));
    // Data Handler (pushes data into database)
    let handle_task = task::spawn(handle_data(database.clone(), db_read_ch, 100));
    // Data Parser (routinely computes statistics on data)
    let parse_task = task::spawn(parse_data(database, 5));

    // Wait for any of the tasks (or ctrl+c) to return.
    tokio::select! {
        res = eventloop_task => {
            if let Err(e) = res {
                println!("Eventloop Error: {}", e);
            }
        }
        res = handle_task => {
            if let Err(e) = res {
                println!("Handle Error: {}", e);
            }
        }
        res = parse_task => {
            if let Err(e) = res {
                println!("Parse Error: {}", e);
            }
        }
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
