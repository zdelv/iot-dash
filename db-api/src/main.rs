use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::net::SocketAddr;
use std::time::Duration;

#[tokio::main()]
async fn main() -> anyhow::Result<()> {
    // Modify environmental variables by reading in from .env file.
    dotenvy::dotenv()?;

    tracing_subscriber::fmt::init();

    // TODO: Actually handle the errors below. ? should really only be used in functions that call
    // from main. Main should handle all errors and display more useful context or attempt to
    // recover.

    // Read in URL to the local database.
    let database_url = match std::env::var("DEV") {
        Ok(_) => std::env::var("DEV_DATABASE_URL")?,
        Err(_) => std::env::var("DATABASE_URL")?,
    };

    let pool = {
        loop {
            let pool = PgPoolOptions::new()
                .max_connections(3)
                .connect(&database_url)
                .await;

            if let Ok(p) = pool {
                break p;
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    };

    let app = Router::new()
        .route("/", get(root))
        .route("/sensor", get(get_sensors))
        .route("/reading", get(get_readings))
        .with_state(pool);
    //.route("/sensor/:sensor_id/reading/:reading_id", get(get_sensor_reading));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3001));
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

#[derive(Serialize, Deserialize)]
struct RootResponse {
    available_endpoints: Vec<String>,
}

#[tracing::instrument]
async fn root() -> (StatusCode, Json<RootResponse>) {
    tracing::debug!("root request");

    let res = RootResponse {
        available_endpoints: vec!["sensor".to_string(), "reading".to_string()],
    };

    (StatusCode::OK, Json(res))
}

#[derive(Serialize)]
struct Return<T>
where
    T: Serialize,
{
    count: i32,
    result: T,
}

impl<T: Serialize> Return<T> {
    fn new(result: T, count: i32) -> Return<T> {
        Return { result, count }
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct SensorsQuery {
    id: Option<i32>,
    gt: Option<i32>,
    lt: Option<i32>,
}

#[tracing::instrument]
async fn get_sensors(
    State(pool): State<PgPool>,
    Query(query): Query<SensorsQuery>,
) -> Result<(StatusCode, Json<Return<Vec<common::Sensor>>>), (StatusCode, Json<ErrorReturn>)> {
    tracing::debug!("get_sensors request");

    // Form SQL query from the incomming REST query.
    // TODO: There must be some way to do this without excessive matching and repeated code.
    let rec = match query {
        SensorsQuery {
            id: Some(id),
            gt: None,
            lt: None,
        } => sqlx::query_as("SELECT * FROM sensors WHERE sensor_id = $1").bind(id),
        SensorsQuery {
            id: None,
            gt: Some(gt),
            lt: None,
        } => sqlx::query_as("SELECT * FROM sensors WHERE sensor_id > $1").bind(gt),
        SensorsQuery {
            id: None,
            gt: None,
            lt: Some(lt),
        } => sqlx::query_as("SELECT * FROM sensors WHERE sensor_id < $1").bind(lt),
        SensorsQuery {
            id: None,
            gt: Some(gt),
            lt: Some(lt),
        } => sqlx::query_as("SELECT * FROM sensors WHERE sensor_id < $1 AND sensor_id > $2")
            .bind(lt)
            .bind(gt),
        _ => return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorReturn::new(
                "Invalid request. Please supply either of gt or lt, both gt and lt, or only id.",
            )),
        )),
    };
    // Perfom the SQL query.
    let rec: Vec<common::Sensor> = rec.fetch_all(&pool).await.map_err(internal_error)?;

    let num_sensors = rec.len() as i32;
    tracing::debug!("Found {} sensors.", num_sensors);

    Ok((StatusCode::OK, Json(Return::new(rec, num_sensors))))
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct ReadingsQuery {
    sensor_id: Option<i32>,
    after: Option<i32>,
    before: Option<i32>,
}

#[tracing::instrument]
async fn get_readings(
    State(pool): State<PgPool>,
    Query(query): Query<ReadingsQuery>,
) -> Result<(StatusCode, Json<Return<Vec<common::Reading>>>), (StatusCode, Json<ErrorReturn>)> {
    tracing::debug!("get_readings request");

    // Form SQL query from the incomming REST query.
    // TODO: There must be some way to do this without excessive matching and repeated code.
    let rec = match query {
        ReadingsQuery {
            sensor_id: Some(sensor_id),
            after: None,
            before: None,
        } => sqlx::query_as("SELECT * FROM stats WHERE sensor_id = $1").bind(sensor_id),
        ReadingsQuery {
            sensor_id: None,
            after: Some(after),
            before: None,
        } => sqlx::query_as("SELECT * FROM stats WHERE timestamp > to_timestamp($1)").bind(after),
        ReadingsQuery {
            sensor_id: None,
            after: None,
            before: Some(before),
        } => sqlx::query_as("SELECT * FROM stats WHERE timestamp < to_timestamp($1)").bind(before),
        ReadingsQuery {
            sensor_id: None,
            after: Some(after),
            before: Some(before),
        } => sqlx::query_as("SELECT * FROM stats WHERE timestamp > to_timestamp($1) AND timestamp < to_timestamp($2)")
            .bind(after)
            .bind(before),
        ReadingsQuery {
            sensor_id: Some(sensor_id),
            after: Some(after),
            before: Some(before),
        } => sqlx::query_as("SELECT * FROM stats WHERE sensor_id = $1 AND timestamp > to_timestamp($2) AND timestamp < to_timestamp($3)")
            .bind(sensor_id)
            .bind(after)
            .bind(before),
        _ => return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorReturn::new(
                "Invalid request. Please supply either of after or before, both after and before, only sensor_id, or all three of after, before, and sensor_id.",
            )),
        )),
    };
    // Perfom the SQL query.
    let rec: Vec<common::Reading> = rec.fetch_all(&pool).await.map_err(internal_error)?;

    let num_readings = rec.len() as i32;
    tracing::debug!("Found {} readings.", num_readings);

    Ok((StatusCode::OK, Json(Return::new(rec, num_readings))))
}

/// JSON Return used for all errors. Wrap this in a JSON before sending.
#[derive(Serialize, Deserialize)]
struct ErrorReturn {
    error: String,
}

impl ErrorReturn {
    fn new(err: &str) -> Self {
        ErrorReturn {
            error: err.to_string(),
        }
    }
}

/// Utility function for mapping any error into a `500 Internal Server Error`
/// response.
/// From axum examples.
#[tracing::instrument]
fn internal_error<E>(err: E) -> (StatusCode, Json<ErrorReturn>)
where
    E: std::error::Error,
{
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorReturn {
            error: err.to_string(),
        }),
    )
}
