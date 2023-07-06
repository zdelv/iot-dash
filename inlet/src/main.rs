use colored::*;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, error::Error, time::Duration};

#[derive(Serialize, Deserialize)]
struct Payload {
    data: f32
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ValType {
    Float,
    Int,
}

#[derive(Serialize, Deserialize)]
struct ValSettings {
    #[serde(rename = "type")]
    _type: ValType,
    min: f32,
    max: f32,
}

#[derive(Serialize, Deserialize)]
struct Sensor {
    count: u16,
    val: ValSettings,
}

#[derive(Serialize, Deserialize)]
struct Location {
    root: String,
    sensors: HashMap<String, Sensor>,
}

async fn setup_mqtt(id: &str, host: &str, port: u16) -> (AsyncClient, EventLoop) {
    let mut mqttoptions = MqttOptions::new(id, host, port);
    mqttoptions.set_keep_alive(Duration::from_secs(5));
    AsyncClient::new(mqttoptions, 10)
}

async fn run_mqtt_eventloop(
    mut eventloop: EventLoop,
    log: bool,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    loop {
        let notification = eventloop.poll().await?;

        if log {
            match notification {
                Event::Incoming(p) => {
                    if let Packet::Publish(pack) = p {
                        println!(
                            "Recieved: Topic: {}, Payload: {:?}",
                            pack.topic,
                            pack.payload.to_vec()
                        );
                    } else {
                        println!("Recieved: {:?}", p);
                    }
                }
                Event::Outgoing(p) => {
                    println!("Sent: {:?}", p);
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::var("SENSOR_TYPES_FILE")?;
    let file = std::fs::File::open(path)?;
    let sensor_settings: HashMap<String, Location> = serde_yaml::from_reader(file)?;

    let (client, eventloop) = setup_mqtt("inlet", "0.0", 1883).await;

    // Eventloop task
    // The eventloop must be running before we begin submitting tasks through client.
    let eventloop_task = tokio::spawn(async move { run_mqtt_eventloop(eventloop, false).await });

    let sensor_names = {
        let capacity = {
            let mut cap = 0;
            for loc in sensor_settings.values() {
                for sensor in loc.sensors.values() {
                    cap += sensor.count;
                }
            }
            cap
        };

        let mut names = Vec::with_capacity(capacity as usize);

        for (name, location) in &sensor_settings {
            println!("Location: {}", format!("{}/{}", location.root, name).blue());
            for (sensor_name, sensor) in &location.sensors {
                println!("\tSensor Name: {}, Count: {}", sensor_name.green(), sensor.count);

                for i in 0..sensor.count {
                    let derived_name =
                        format!("{}/{}/{}/{}", location.root, name, sensor_name, i);

                    names.push(derived_name);
                }
            }
        }
        names
    };

    // let mut num_attempts = 0;
    // for name in &sensor_names {
    //     while num_attempts < 3 {
    //         match client.subscribe(name, QoS::AtMostOnce).await {
    //             Ok(_) => break,
    //             Err(e) => {
    //                 println!("Error subscribing to {}. Reattempting.", name.blue());
    //                 println!("Error: {}", e.to_string().red());
    //                 num_attempts += 1;
    //             }
    //         }
    //     }
    //     if num_attempts == 3 {
    //         println!("Failed to subscribe to {name} {num_attempts} times. Exiting early.");
    //         std::process::exit(1);
    //     }
    // }

    // Publish task
    // TODO: Change this to either one task per sensor type, or one task per sensor
    const PUB_COUNT: u16 = 10_000;
    let publish_task: tokio::task::JoinHandle<Result<(), Box<dyn Error + Send + Sync>>> =
        tokio::spawn(async move {
            for _ in 1..PUB_COUNT {
                for sensor in &sensor_names {
                    let data = bincode::serialize(&Payload { data: rand::random() })?;
                    client
                        .publish(sensor, QoS::AtLeastOnce, false, data)
                        .await?;
                    // tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
            Ok(())
        });
    println!("Publishing {} times per sensor...", PUB_COUNT);

    // Await both tasks for their results.
    // We only care about the errors, so just return them here if they occur.
    tokio::select! {
        res = publish_task => {
            if let Err(e) = res? {
                println!("Publish Error: {}", e);
            }
        }
        res = eventloop_task => {
            if let Err(e) = res? {
                println!("Eventloop Error: {}", e);
            }
        }
        res = tokio::signal::ctrl_c() => {
            if let Err(e) = res {
                println!("Signal Error: {}", e);
            } else {
                println!("Caught ctrl+c, exiting.");
            }
        }
    }

    Ok(())
}
