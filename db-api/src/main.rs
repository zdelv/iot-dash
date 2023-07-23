mod endpoints;

use sqlx::postgres::PgPoolOptions;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

use crate::endpoints::app;

#[tokio::main()]
async fn main() -> anyhow::Result<()> {
    // Modify environmental variables by reading in from .env file.
    dotenvy::dotenv()?;

    // Log filter follows env-filter formats. See tracing_subscriber for more info.
    let log_filter = std::env::var("LOG_FILTER").unwrap_or("db_api=info".to_string());
    let log_dir = std::env::var("LOG_DIR").unwrap_or("./".to_string());

    // Create a file appender that automatically outputs all logs to a file, with hourly rollover.
    let file_appender = tracing_appender::rolling::hourly(log_dir, "db-api.log");
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

    // Read in URL to the local database.
    let database_url = match std::env::var("POD") {
        Ok(_) => std::env::var("POD_DATABASE_URL")?,
        Err(_) => std::env::var("DATABASE_URL")?,
    };

    let port = std::env::var("PORT")
        .unwrap_or("3001".to_string())
        .parse::<u16>()?;

    let addr = std::env::var("ADDR").unwrap_or("0.0.0.0".to_string());

    let pool = {
        loop {
            tracing::info!("Attempting to connect to database: {}", database_url);
            let pool = PgPoolOptions::new()
                .max_connections(3)
                .connect(&database_url)
                .await;

            match pool {
                Ok(p) => break p,
                Err(e) => tracing::error!("Failed to connect to database: {}", e),
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    };

    let app = app(pool);

    let addr = SocketAddr::from((addr.parse::<Ipv4Addr>()?, port));
    tracing::info!("Listening on {}", addr);

    let server_task = axum::Server::bind(&addr).serve(app.into_make_service());

    tokio::select! {
        res = server_task => {
            tracing::info!("Server returned!");
            if let Err(e) = res {
                tracing::info!("Server Error: {}", e);
            }
        }
        res = tokio::signal::ctrl_c() => {
            if let Err(e) = res {
                tracing::info!("Signal Error: {}", e);
            } else {
                tracing::info!("Caught ctrl+c, exiting.");
            }
        }
    }

    Ok(())
}
