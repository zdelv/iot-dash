mod actors;
mod connection;

use anyhow::Context;
use rumqttc::{AsyncClient, MqttOptions, QoS};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, RwLock},
    task,
};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

use crate::actors::*;
use crate::connection::{DbApiClient, DbApiConnection};

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
            .with(fmt::Layer::default().with_writer(file_writer)),
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
    let http_client = DbApiClient::new(reqwest::Client::new(), &db_api_hostname);

    // Attempt to connect to the db-api before continuing.
    loop {
        tracing::info!("Attempting to conenct to db-api: {}", &db_api_hostname);

        if http_client.is_connection_up().await {
            break;
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    // Map of topics mapped to historical values.
    let database: Arc<RwLock<SensorDataMap>> = Arc::new(RwLock::new(HashMap::new()));
    // Channel used to transfer messages between eventloop and handler
    let (db_write_ch, db_read_ch) = mpsc::channel::<ChannelMessage>(100);

    // Token used to cancel tasks. All tasks recieve this token and are setup to cancel when the
    // token is canelled.
    let token = CancellationToken::new();
    // Shutdown channel that is not read or received from (for data at least). This is used to
    // signal when our tasks have gracefully exited. We do not cancel them with unfinished state.
    let (shutdown_send, mut shutdown_recv) = mpsc::channel(1);

    // MQTT Eventloop (pushes recieved events to data handler)
    let eventloop_task = task::spawn(run_eventloop(
        eventloop,
        db_write_ch,
        token.clone(),
        shutdown_send.clone(),
    ));
    // Data Handler (pushes data into database)
    let handle_task = task::spawn(handle_data(
        database.clone(),
        db_read_ch,
        100,
        token.clone(),
        shutdown_send.clone(),
    ));
    // Data Parser (routinely computes statistics on data)
    let parse_task = task::spawn(parse_data(
        database,
        http_client,
        sensor_infos,
        interval as u64,
        token.clone(),
        shutdown_send,
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
        }
    }
    tracing::info!("Starting shutdown. Waiting for tasks to complete. If 10 seconds pass, force shutdown will occur. Press ctrl+c again to start force shutdown immediately.");

    // Send the cancellation to the token.
    token.cancel();

    // Wait for the channel to close for a graceful shutdown. If 10 seconds pass, we forcefully
    // cancel the tasks (drop them completely). The use can also press ctrl+c to kill the program
    // before 10 seconds are up.
    tokio::select! {
        _ = shutdown_recv.recv() => {}
        _ = tokio::time::sleep(Duration::from_secs(10)) => {
            tracing::error!("A task did not properly shutdown, forcing shutdown!");
            Err(anyhow::anyhow!("Failed to properly shutdown!"))?
        }
        res = tokio::signal::ctrl_c() => {
            if let Err(e) = res {
                tracing::error!("Signal Error: {}", e);
            } else {
                tracing::error!("Caught ctrl+c again, forcefully dropping tasks.");
            }
            Err(anyhow::anyhow!("Failed to properly shutdown!"))?
        }
    }

    Ok(())
}
