use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPool, Pool, Postgres};

pub fn app(pool: Pool<Postgres>) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/sensor", get(get_sensors))
        .route("/sensor", post(post_sensors))
        .route("/reading", get(get_readings))
        .route("/reading", post(post_readings))
        .fallback(fallback)
        .with_state(pool)
}

#[derive(Serialize, Deserialize)]
pub struct RootResponse {
    available_endpoints: Vec<String>,
}

/// Response recieved when going to the root URL.
///
/// Currently only returns the available endpoints.
#[tracing::instrument]
pub async fn root() -> (StatusCode, Json<RootResponse>) {
    tracing::debug!("root request");

    let res = RootResponse {
        available_endpoints: vec!["sensor".to_string(), "reading".to_string()],
    };

    (StatusCode::OK, Json(res))
}

/// Main return struct used for all non-error returns. Result is designed to be a Json-serialized
/// type. No restrictions on T exist, except for Serialize.
#[derive(Serialize)]
pub struct Return<T> {
    count: i32,
    result: T,
}

impl<T: Serialize> Return<T> {
    fn new(result: T, count: i32) -> Return<T> {
        Return { result, count }
    }
}

/// The query when a GET is recieved on the /sensor endpoint
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct SensorsQuery {
    id: Option<i32>,
    gt: Option<i32>,
    lt: Option<i32>,
}

/// The return from a GET on the /sensor endpoint
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct SensorsReturn {
    sensors: Vec<common::Sensor>,
}

type GetSensorsReturn = Result<(StatusCode, Json<Return<SensorsReturn>>), ResultErrorReturn>;

/// Endpoint handling GET /sensor
///
/// This should return a list of sensors matching what is in the database given the query.
#[tracing::instrument]
pub async fn get_sensors(
    Query(query): Query<SensorsQuery>,
    State(pool): State<PgPool>,
) -> GetSensorsReturn {
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
    let sensors: Vec<common::Sensor> = rec.fetch_all(&pool).await.map_err(internal_error)?;

    let num_sensors = sensors.len() as i32;
    tracing::debug!("Found {} sensors.", num_sensors);

    Ok((
        StatusCode::OK,
        Json(Return::new(SensorsReturn { sensors }, num_sensors)),
    ))
}

///
/// POST /sensor should have a JSON payload of:
/// { "topics": ["topic1", "topic2", ...]}
///
/// POST /sensor will return a JSON payload of:
/// { "count": 123, "results": { "reading_ids": [123, ...] }
///

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct SensorsPost {
    topics: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct SensorsPostReturn {
    sensor_ids: Vec<i32>,
}

type PostSensorsReturn = Result<(StatusCode, Json<Return<SensorsPostReturn>>), ResultErrorReturn>;

/// Endpoint handling POST /sensor
///
/// See above for more information on the JSON payload.
#[tracing::instrument]
pub async fn post_sensors(
    State(pool): State<PgPool>,
    Json(input): Json<SensorsPost>,
) -> PostSensorsReturn {
    tracing::debug!("post_sensors request");

    if input.topics.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorReturn::new("No valid topics provided.")),
        ));
    }

    let sensor_ids: Vec<i32> = sqlx::query_scalar(
        r#"INSERT INTO sensors (topic)
        SELECT * FROM UNNEST($1::varchar(255)[])
        RETURNING sensor_id"#,
    )
    .bind(input.topics)
    .fetch_all(&pool)
    .await
    .map_err(internal_error)?;

    let num_sensors = sensor_ids.len();
    tracing::debug!("{} sensors created.", num_sensors);

    Ok((
        StatusCode::CREATED,
        Json(Return::new(
            SensorsPostReturn { sensor_ids },
            num_sensors as i32,
        )),
    ))
}

/// The query that is recieved from a GET request on the /reading endpoint
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ReadingsQuery {
    sensor_id: Option<i32>,
    after: Option<i32>,
    before: Option<i32>,
}

/// The return from a GET request on the /reading endpoint.
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ReadingsReturn {
    readings: Vec<common::Reading>,
}

type GetReadingsReturn = Result<(StatusCode, Json<Return<ReadingsReturn>>), ResultErrorReturn>;

/// Endpoint handling GET /reading
///
/// This should return a list of readings matching what is in the database given the query.
#[tracing::instrument]
pub async fn get_readings(
    State(pool): State<PgPool>,
    Query(query): Query<ReadingsQuery>,
) -> GetReadingsReturn {
    tracing::debug!("get_readings request");

    // Form SQL query from the incomming REST query.
    // TODO: There must be some way to do this without excessive matching and repeated code.
    let rec = match query {
        ReadingsQuery {
            sensor_id: Some(sensor_id),
            after: None,
            before: None,
        } => sqlx::query_as("SELECT * FROM readings WHERE sensor_id = $1").bind(sensor_id),
        ReadingsQuery {
            sensor_id: None,
            after: Some(after),
            before: None,
        } => sqlx::query_as("SELECT * FROM readings WHERE timestamp > to_timestamp($1)").bind(after),
        ReadingsQuery {
            sensor_id: None,
            after: None,
            before: Some(before),
        } => sqlx::query_as("SELECT * FROM readings WHERE timestamp < to_timestamp($1)").bind(before),
        ReadingsQuery {
            sensor_id: None,
            after: Some(after),
            before: Some(before),
        } => sqlx::query_as("SELECT * FROM readings WHERE timestamp > to_timestamp($1) AND timestamp < to_timestamp($2)")
            .bind(after)
            .bind(before),
        ReadingsQuery {
            sensor_id: Some(sensor_id),
            after: Some(after),
            before: Some(before),
        } => sqlx::query_as("SELECT * FROM readings WHERE sensor_id = $1 AND timestamp > to_timestamp($2) AND timestamp < to_timestamp($3)")
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
    let readings: Vec<common::Reading> = rec.fetch_all(&pool).await.map_err(internal_error)?;

    let num_readings = readings.len() as i32;
    tracing::debug!("Found {} readings.", num_readings);

    Ok((
        StatusCode::OK,
        Json(Return::new(ReadingsReturn { readings }, num_readings)),
    ))
}

///
/// POST /reading should have a JSON payload of:
/// { "readings": [
///     { "sensor_id": 123, "reading_type": ["average", "minimum"], "reading": [10.2, 1.0] }
///     ...
/// ]}
///
/// POST /readings will return a JSON payload of:
/// { "count": 123, "results": { "reading_ids": [123, ...] }
///

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ReadingsPost {
    readings: Vec<ReadingsPostItem>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct ReadingsPostItem {
    sensor_id: i32,
    reading_type: Vec<common::ReadingType>,
    reading: Vec<f32>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ReadingsPostReturn {
    reading_ids: Vec<i32>,
}

type PostReadingsReturn = Result<(StatusCode, Json<Return<ReadingsPostReturn>>), ResultErrorReturn>;

/// Endpoint handling POST /reading
///
/// See above for more information on the JSON payload.
#[tracing::instrument]
pub async fn post_readings(
    State(pool): State<PgPool>,
    Json(input): Json<ReadingsPost>,
) -> PostReadingsReturn {
    tracing::debug!("post_readings request");

    if input.readings.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorReturn::new("No valid readings provided.")),
        ));
    }

    // As of sqlx 7.1, inserting multidimensional arrays into a database is not supported. We would
    // like to do that here using UNNEST() with an array of arrays for both reading_type and
    // reading. This below loop is technically much slower due to it being many transactions.
    let mut reading_ids = Vec::with_capacity(input.readings.len());
    for reading in input.readings {
        let id: i32 = sqlx::query_scalar(
            r#"INSERT INTO readings (sensor_id, type, reading)
            VALUES ($1, $2::reading_type[], $3::REAL[])
            RETURNING reading_id"#,
        )
        .bind(reading.sensor_id)
        .bind(reading.reading_type)
        .bind(reading.reading)
        .fetch_one(&pool)
        .await
        .map_err(internal_error)?;
        reading_ids.push(id);
    }

    let num_readings = reading_ids.len();
    tracing::debug!("{} readings created.", num_readings);

    Ok((
        StatusCode::CREATED,
        Json(Return::new(
            ReadingsPostReturn { reading_ids },
            num_readings as i32,
        )),
    ))
}

type ResultErrorReturn = (StatusCode, Json<ErrorReturn>);

/// JSON Return used for all errors. Wrap this in a JSON before sending.
#[derive(Serialize, Deserialize)]
pub struct ErrorReturn {
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

/// Fallback for when no route matches.
pub async fn fallback(uri: axum::http::Uri) -> (StatusCode, String) {
    tracing::info!("Request to unknown endpoint: {}", uri);
    (
        StatusCode::NOT_FOUND,
        format!("No endpoint found matching {}", uri),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http, http::Request};
    use serde_json::json;
    use sqlx::{Pool, Postgres};
    use tower::ServiceExt;

    #[sqlx::test]
    async fn i_am_root(pool: Pool<Postgres>) {
        let app = app(pool);

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body, json!({"available_endpoints": ["sensor", "reading"]}));
    }

    #[sqlx::test]
    async fn check_fallback(pool: Pool<Postgres>) {
        let app = app(pool);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/does-not-exist")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();

        assert_eq!(&body[..], b"No endpoint found matching /does-not-exist");
    }

    #[sqlx::test(fixtures("sensors", "readings"))]
    async fn can_get_sensor_gt_lt(pool: Pool<Postgres>) {
        let app = app(pool);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sensor?gt=1&lt=4")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let exp_body = json!({
            "count": 2,
            "result": {
                "sensors": [
                    { "sensor_id": 2, "topic": "/this/is/a/topic" },
                    { "sensor_id": 3, "topic": "/home/livingroom/light/1" },
                ]
            }
        });

        assert_eq!(body, exp_body);
    }

    #[sqlx::test(fixtures("sensors", "readings"))]
    async fn can_get_sensor_id(pool: Pool<Postgres>) {
        let app = app(pool);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sensor?id=4")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let exp_body = json!({
            "count": 1,
            "result": {
                "sensors": [
                    { "sensor_id": 4, "topic": "/home/livingroom/light/2" },
                ]
            }
        });

        assert_eq!(body, exp_body);
    }

    #[sqlx::test(fixtures("sensors", "readings"))]
    async fn can_post_sensor(pool: Pool<Postgres>) {
        let app = app(pool);

        let post_body = json!({
            "topics": ["/another/topic", "/yet/another/topic"]
        });

        let post_body = serde_json::to_string(&post_body).unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/sensor")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(post_body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let exp_body = json!({
            "count": 2,
            "result": {
                "sensor_ids": [5, 6]
            }
        });

        assert_eq!(body, exp_body);
    }

    #[sqlx::test(fixtures("sensors", "readings"))]
    async fn can_get_reading_all(pool: Pool<Postgres>) {
        let app = app(pool);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/reading?sensor_id=4&after=1690127300&before=1690127430")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let exp_body = json!({
            "count": 3,
            "result": {
                "readings": [
                    {
                        "reading_id": 4,
                        "sensor_id": 4,
                        "timestamp": 1690127400,
                        "type": ["median", "count"],
                        "reading": [15.1, 10.0]
                    },
                    {
                        "reading_id": 5,
                        "sensor_id": 4,
                        "timestamp": 1690127410,
                        "type": ["median", "count"],
                        "reading": [25.2, 20.0]
                    },
                    {
                        "reading_id": 6,
                        "sensor_id": 4,
                        "timestamp": 1690127420,
                        "type": ["median", "count"],
                        "reading": [35.3, 30.0]
                    }
                ]
            }
        });

        assert_eq!(body, exp_body);
    }

    #[sqlx::test(fixtures("sensors", "readings"))]
    async fn can_post_reading(pool: Pool<Postgres>) {
        let app = app(pool);

        let post_body = json!({
            "readings": [
                {"sensor_id": 3, "reading_type": ["average", "maximum"], "reading": [10.2, 100.5]},
                {"sensor_id": 4, "reading_type": ["minimum", "count"], "reading": [12.2, 100.0]},
                {"sensor_id": 1, "reading_type": ["median", "maximum", "minimum"], "reading": [15.1, 10.5, 1.0]},
            ]
        });

        let post_body = serde_json::to_string(&post_body).unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/reading")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(post_body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let exp_body = json!({
            "count": 3,
            "result": {
                "reading_ids": [7, 8, 9]
            }
        });

        assert_eq!(body, exp_body);
    }
}
