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
        <td>`id`</td>
        <td>Gets the sensor matching this id. Cannot be used with `lt` or `gt`.</td>
    </tr>
    <tr>
        <td>`gt`</td>
        <td>Gets the sensor greater than this id. Can be used with `lt`, but not `id`.</td>
    </tr>
    <tr>
        <td>`lt`</td>
        <td>Gets the sensor less than this id. Can be used with `gt`, but not `id`.</td>
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
