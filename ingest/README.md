# `ingest`

A service handling ingest of raw sensor data via MQTT and submission to a local
database. Ingested data is collated and data reduction statistics per sensor
are performed prior to submission. This is done to reduce the total amount of
data being saved in the database (lowering data growth and overall cost over
time).

## Usage

`ingest` requires a configuration file to setup how each recieved sensor is
collated. An example config file is available under `config.yaml`. This config
should be specified using the `CONFIG` environmental variable. Other than the
config, `ingest` can be run as a standard Rust binary:

```bash
export CONFIG=config.yaml
cargo run --release
```

You may also specify both the log level and log directory through the `.env`
file (or as environmental variables). `LOG_DIR` specifies the directory, while
`LOG_FILTER` specifies the level. By default, `LOG_DIR` is set to the current
directory, and `LOG_FILTER` is set to debug only `ingest`.

The `.env` file also includes hostnames for the MQTT broker and database REST
API (`db-api`). When running outside of a pod, `DB_HOSTNAME` and
`MQTT_HOSTNAME` are used. When running as a part of a larger pod, set the `POD`
environmental variable (the value is irrelevant), and `ingest` will read the
`POD_DB_HOSTNAME` and `POD_MQTT_HOSTNAME` variables instead. The non `POD_`
variables are designed for testing outside of the pod, while the `POD_`
variables are designed for within the pod.

## Config

The configuration file has the following format:

```yaml
interval: <interval_time>
sensors:
- name: <sensor_name>
  topic: <sensor_topic_regex>
  operations:
    - <operation>
```

In the above example configuration, the fields have the following meanings:

- `<interval_time>`
    - The interval of time between two successive data collations and
        submissions to the DB (if data exists).
    - Units of seconds.
- `<sensor_name>`
    - The internal name for this sensor. This is not used currently and is only
        meant for debugging and labeling purposes.
- `<sensor_topic_regex>`
    - A regex pattern used to match this sensor onto recieved sensor topics.
    - The `ingest` tool will only apply the operations specified by this sensor
        if it matches this regex pattern.
- `<operation>`
    - An operation that is performed on the data recieved for this sensor (every `<interval_time>` second).
    - The available operations are: `average`, `minimum`, `maximum`, `median`, `count`
    - This is a list, with any number of operations allowed.
    - At least one operation must be supplied for data to be published for this sensor.

Any number of sensors can be defined in the config. An example configuration is the following:

```yaml
interval: 5
sensors:
- name: living_room_temp
  topic: /home/living_room/temp/\d+
  operations:
    - minimum
    - maximum
    - median
- name: garage_door_motion
  topic: /home/garage_door/motion/\d+
  operations:
    - count
```
