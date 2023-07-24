# `db-api`

The REST API service wrapping the PostgreSQL database. This is designed to be
the sole service that communicates directly with the database. All other
services that need access to the database should communicate with this service
via its REST API. The API is described below.

## Usage

This is not meant to be run standalone, and instead should be run as a part of
the larger IoT Dashboard pod. See the root README for more information on this.

Starting the `db-api` service outside of a pod requires nothing more than
running the binary:

```bash
cargo run --release
```

If the service is being run within a pod, ensure that the `.env` file is in the
same directory as the binary is started, and that the `POD` environmental
variable is set to some value (the value is irrelevant). The `POD`
environmental variable tells the service to use the `POD_DATABASE_URL` instead
of the `DATABASE_URL`, which exists for testing w/ SQLx.

By default `db-api` is set to a log level of `debug`, which includes all
logging currently. To change this, modify the `LOG_FILTER` variable in the
`.env` file. The `LOG_DIR` environmental variable can also be used to set where
the log should be saved. If `LOG_DIR` is not supplied, the log is placed in the
current directory.


## Testing

It is required that you have previously created the pod for the IoT Dashboard
platform. See the root README for more information. 

Assuming the pod is currently not running, run the following commands prior to
running tests:

```bash
bash podman start db adminer
```

This will start the Postgres database as well as the Adminer interface (to help
debug test failures). You may now run the tests using the standard `cargo
test`.

## Endpoints

### `GET /sensor`

Gets sensors saved in the database given filters.

#### Filters:

<table>
    <tr>
        <th>Filter</th>
        <th>Description</th>
    </tr>
    <tr>
        <td><code>id</code></td>
        <td>Gets the sensor matching this id. Cannot be used with <code>lt</code> or <code>gt</code>.</td>
    </tr>
    <tr>
        <td><code>gt</code></td>
        <td>Gets the sensor greater than this id. Can be used with <code>lt</code>, but not <code>id</code>.</td>
    </tr>
    <tr>
        <td><code>lt</code></td>
        <td>Gets the sensor less than this id. Can be used with <code>gt</code>, but not <code>id</code>.</td>
    </tr>
</table>


#### Example:

_Request_:
```
http://localhost:3001/sensor?id=10
```

_Return_:
```json
{
    "count": 1,
    "result": {
        "sensors": [
            { "sensor_id": 10, "topic": "/a/sensor/topic" }
        ]
    }
}
```

### `POST /sensor`

Adds new sensors given a list of topics.

#### Example:


_URL_
```
http://localhost:3001/sensor
```

_JSON POST Body_:
```json
{
    "topics": [
        "/a/topic",
        "/another/topic"
    ]
}
```

_Return_:
```json
{
    "count": 2,
    "result": {
        "sensor_ids": [1, 2]
    }
}
```


### `GET /reading`

Gets readings saved in the database given filters.

#### Filters:

<table>
    <tr>
        <th>Filter</th>
        <th>Description</th>
    </tr>
    <tr>
        <td><code>sensor_id</code></td>
        <td>Gets the readings with a matching sensor_id.</td>
    </tr>
    <tr>
        <td><code>after</code></td>
        <td>Gets readings after a certain time. Must be a Unix timestamp (in seconds).</td>
    </tr>
    <tr>
        <td><code>before</code></td>
        <td>Gets readings before a certain time. Must be a Unix timestamp (in seconds).</td>
    </tr>
</table>

All filters can be used simultaneously. All `reading`s returned will be floating points.

The return will contain a `reading_type` field. The possible values in the list are:

<table>
    <tr>
        <th>Reading Type</th>
        <th>Description</th>
    </tr>
    <tr>
        <td><code>count</code></td>
        <td>The number of sensor readings over the interval.</td>
    </tr>
    <tr>
        <td><code>average</code></td>
        <td>The average of the sensor readings over the interval.</td>
    </tr>
    <tr>
        <td><code>median</code></td>
        <td>The median of the sensor readings over the interval.</td>
    </tr>
    <tr>
        <td><code>minimum</code></td>
        <td>The minimum of the sensor readings over the interval.</td>
    </tr>
    <tr>
        <td><code>maximum</code></td>
        <td>The maximum of the sensor readings over the interval.</td>
    </tr>
</table>


#### Example:

_Request_:
```
http://localhost:3001/reading?sensor_id=10&after=1689865196
```

_Return_:
```json
{
    "count": 2,
    "result": {
        "readings": [
            {
                "reading_id": 123,
                "sensor_id": 10,
                "timestamp": 1689865200,
                "reading_type": ["count", "average"],
                "reading": [10.0, 125.4]
            },
            {
                "reading_id": 128,
                "sensor_id": 10,
                "timestamp": 1689865800,
                "reading_type": ["count", "average"],
                "reading": [2.0, 100.2]
            }
        ]
    }
}
```

### `POST /reading`

Adds new readings for a given sensor.

See above for the possible values of `reading_type`. Each reading is a floating
point matching with the reading type in the `reading_type` field (at the same
index).

#### Example:


_URL_
```
http://localhost:3001/reading
```

_JSON POST Body_:
```json
{
    "readings": [
        {
            "sensor_id": 10,
            "reading_type": ["count", "average"],
            "reading": [12.0, 100.3]
        },
        {
            "sensor_id": 10,
            "reading_type": ["count", "average"],
            "reading": [12.0, 100.3]
        }
    ]
}
```

_Return_:
```json
{
    "count": 2,
    "result": {
        "reading_ids": [129, 130]
    }
}
```
