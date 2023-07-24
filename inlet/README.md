# `inlet`

A small binary that acts as a set of sensors all publishing to the MQTT broker
under different topics. The name `inlet` is due to this being the main
entrypoint for new data into the platform. In a real deployment, this service
would be replaced with a large collection of sensors.

## Usage

A sensor config file must be specified before running `inlet`. A default config
is found in `sensors.yaml`. A path to the config should be available as an
environmental variable named `SENSOR_TYPES_FILE`. From there, `inlet` can be
run like a normal Rust binary:

```bash
export SENSOR_TYPES_FILE=sensors.yaml
cargo run --release
```

## Config

The config file has the following format:

```yaml
<location>:
    root: <root_location>
    sensors:
        <sensor_name>:
            count: <num_sensors>
            val:
                type: <val_type>
                min: <minimum>
                max: <maximum>
```

This structure defines both the topic path used for submitting to the MQTT
broker and the range + type of the data submitted. In the above, the following
topic would be formed: `<root_location>/<location>/<sensor_name>/<num>`, where
all variables map to the above and `<num>` is an integer between 0 and
`<num_sensors>`. An example topic is `/home/livingroom/temp/10`.

The `val` section in a sensor defines the data type and minimum and maximum
values submitted by the sensor. The data type is currently ignore and is always
a float. Minimum and maximum are used for generating a random number within the
range.

Multiple locations can be defined in a single file as well as multiple sensors
per location.

An example location is the following:

```yaml
kitchen:
    root: home
    sensors:
        temp:
            count: 10
            val:
                type: float
                min: 20.0
                max: 150.0
        motion:
            count: 5
            val: 
                type: float
                min: 1.0
                max: 10.0
```
