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
