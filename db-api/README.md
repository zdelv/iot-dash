# `db-api`

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
