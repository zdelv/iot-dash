use colored::*;
use rand::Rng;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, error::Error, time::Duration};

#[derive(Serialize)]
struct Payload {
    data: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum ValType {
    Float,
    Int,
}

#[derive(Deserialize)]
struct ValSettings {
    #[serde(rename = "type")]
    _type: ValType,
    min: f32,
    max: f32,
}

#[derive(Deserialize)]
struct Sensor {
    count: u16,
    val: ValSettings,
    name: Option<String>,
}

#[derive(Deserialize)]
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

    // Load in the sensor seettings struct from the yaml file.
    let mut sensor_settings: HashMap<String, Location> = serde_yaml::from_reader(file)?;

    // Iterate through the settings and fill in the name field. This is created from the root + the
    // location name + the sensor name.
    for (name, location) in &mut sensor_settings {
        println!("Location: {}", format!("{}/{}", location.root, name).blue());
        for (sensor_name, sensor) in &mut location.sensors {
            println!(
                "\tSensor Name: {}, Count: {}",
                sensor_name.green(),
                sensor.count
            );

            let derived_name = format!("{}/{}/{}", location.root, name, sensor_name);
            sensor.name = Some(derived_name);
        }
    }

    // Create the MQTT eventloop
    let (client, eventloop) = setup_mqtt("inlet", "0.0", 1883).await;

    // Eventloop task
    // The eventloop must be running before we begin submitting tasks through client.
    let eventloop_task = tokio::spawn(async move { run_mqtt_eventloop(eventloop, false).await });

    // Publish task
    // Iterate through all sensors, publishing PUB_COUNT times to each. Rotate through each rather
    // than publish all PUB_COUNT to one sensor at a time.
    const PUB_COUNT: u16 = 10_000;
    let publish_task: tokio::task::JoinHandle<Result<(), Box<dyn Error + Send + Sync>>> =
        tokio::spawn(async move {
            // Publish PUB_COUNT times to all sensors.
            for _ in 0..PUB_COUNT {
                for location in sensor_settings.values() {
                    for sensor in location.sensors.values() {
                        for j in 0..sensor.count {
                            let data = bincode::serialize(&Payload {
                                data: rand::thread_rng().gen_range(sensor.val.min..sensor.val.max),
                            })?;
                            let name = format!("{}/{}", sensor.name.as_ref().unwrap(), j);

                            client.publish(&name, QoS::AtLeastOnce, false, data).await?;
                        }
                    }
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
