# IoT Dashboard

IoT Dashboard is a proof-of-concept project of a "full" platform for an IoT
ingest and dashboarding system. IoT in this case is defined as a set of small
internet-connected sensors that transmit small packets at a high frequency,
almost immediately after data acquisition. This platform starts by taking in raw
data from sensors over some transport layer (MQTT), then performing real-time
analytics and submitting to a database (PostgreSQL). The database is wrapped
with a REST API that supports GET and POST requests for sensor information and
readings. A front-end can then be built on top of this database API for
information retrieval.

All of this is designed to be containerized and can run semi-independently. Some
requirements exist, like the connection between the real-time analytics
component and the database, or the REST API and the database. All container
orchestration is done via Podman, with shell scripts for defining the Pod.

Expected uses of the platform are the following:

- A small home with a small amount of DIY (or off-the-shelf) sensors.
    - Sensor types might include temperature, CO&#8322;, occupancy, and soil moisture.
    - This may also include toggleable sensors like door sensors.
- A large factory with thousands of sensors across a manufacturing line.
    - This is somewhat similar to the small home user, but with just much higher
        sensor counts and stricter requirements on data storage and alerting.
    - The realtime analytics can be used to alert for faults as well as perform
        statistics on overall yields or equipment uptime.
    - A dashboard could also be used by non-engineering (business, finance, etc)
        members for reporting purposes. (Not implemented here)


## Usage

### Prerequisites

* Podman
* A Linux-compatibile device
    * Development was done on MacOS, with deployment tested on Arch Linux.

### Steps

Clone this repo and enter the root of the directory:

```bash
git clone https://git.sr.ht/~rael/iot_dash
cd iot_dash
```

Create the pod by running `create_pod.sh` and providing a pod name as the first
argument:

```bash
./create_pod.sh iot_dash_pod
```

Start the pod using `podman`:

```bash
podman pod start iot_dash_pod
```

Monitor the pod's startup (the pod should not say degraded):

```bash
podman ps --pod
```

Pod resource usage can be found with:

```bash
podman pod stats
```

Stop the pod:

```bash
podman pod stop iot_dash_pod
```

Sometimes this doesn't actually stop the pod (check with `podman ps --pod`), so manually kill all containers:

```bash
podman pod stop mqtt db adminer db-api ingest
```

### Using the endpoints

There are two main endpoints: the MQTT broker and db-api. The MQTT broker
handles input into the system, while db-api provides an API to access
information hosted internally.

To start, lets add data into the system using the `inlet` program. `inlet`
connects via MQTT to the broker and sends packets of data under many different
sensor names. This simulates having many real-world senors publishing
simultaneously (somewhat).

To run `inlet`, do the following:

```bash
cd inlet
export SENSOR_TYPES_FILE=sensors.yaml
cargo run
```

The `SENSOR_TYPES_FILE` environmental variable sets the path to the config
`inlet` uses to setup. This contains information about the sensors it sends
data as. When running `inlet`, it should display information about what MQTT
topics it will be publishing as. It then begins to stream 10,000 packets per
sensor, jumping between each of the sensors in the process.

When `inlet` finishes running, the database should now have data in it. To
check this, we can use the `db-api`. The `db-api` should be exposed under port
`3001`. Run the following to test this:

```bash
curl http://localhost:3001
```

You should recieve a packet containing a JSON struct like the following:
`{"available_endpoints": ["sensor", "reading"]}`. These two endpoints should be
available to use.

Lets try the `sensor` endpoint:

```bash
curl http://localhost:3001/sensor?gt=0
```

This should return all sensors with an ID greater than 0, which is all sensors.
Sensors are assigned an ID by the database as they connect to the MQTT broker
and send their first packet. The return should also include the topic of each
sensor.

Now lets look at the `reading` enpoint:

```bash
curl http://localhost:3001/reading?sensor_id=10
```

This returns all readings in the database with a `sensor_id` of 10. You can
replace this value with any sensor found in the previous `sensor` endpoint
command to get different numbers.

You can also filter off of the reading submission time. Internally, the
database attaches a timestamp to each reading, marking the time that the
reading was submitted into the database (not when the reading was taken). The
`reading` endpoint also has `before` and `after` filters that can be used with
a Unix timestamp to find all readings before or after a certain time.

From the previous `reading` request, find two readings separated in time, grab
the timestamps and enter them into the request below, with `before` being the
larger timestamp and `after` being the smaller timestamp:

```bash
curl http://localhost:3001/reading?before=<timestamp>&after=<timestamp>&sensor_id=10
```

You should get back all readings between those two timestamps. We also further
queried on just readings between those times from sensors with a `sensor_id` of
10.

The `db-api` also has `POST` support for adding sensors and readings. The
`ingest` tool uses these, and new tools can be built off of them. Full endpoint
documentation for `db-api` can be found in it's README file. 

### Testing

Full scale testing for this project is still a work-in-progress. Currently,
`db-api` has a full suite of unit tests thanks to SQLx and it's great DB
testing support, as well as Axum and Tower's easy to use offline service
handlers. `ingest` also is able to be unit tested without any external
services.

The unit tests that currently exist can be run with `cargo test`. These require
that the Postgres database be running. After going through the above setup
procedure for creating the pod, you should be able to start the database with
the following command:

```bash
podman start db
```

Then run the tests (from the root `iot_dash` directory):

```bash
cargo test
```

You may also start `adminer` in the same way, which will give you an admin
interface into the database. By default, SQLx leaves any databases around after
a panic (test failure). Adminer allows you to investigate test failures fairly
easily.

## Architecture

The three goals for development of this platform, as well as the rationale behind
them, are:

1. Low latency
    - Faults on a production line or a garage door being open at an unexpected
        time require quick alerting to prevent downstream issues from occurring.
        An ingest application allows for configuration of the analytics and
        alerting from realtime data (alerting not yet implemented).
2. Configurablity
    - No single setup will work with all applications. Some degree of
        configuration to each section of the platform will allow for wider
        breadth of usecases.
    - Configurablity is a double-edge sword though. Providing a configuration
        parameter for every possible item makes setup a nightmare, and at some
        point makes the platform less-desirable. Out-of-the-box configurations
        for a few key usecases is an important requirement.
3. Ease of setup
    - The platform should be able to be stood up on commodity hardware running
        in a home, by a single person. It should also be flexible enough to
        allow a team of engineers to extend and modify without losing much of
        the ease of setup.

All three goals directly influenced multiple design decisions:

1. Sensor packet size (low latency)
    - Each sensor must be designed to submit packets with a single floating
        point as its only field. This may seem limiting, but to help improve
        latency, the sensors are encouraged to submit many small packets
        containing one reading rather than few large packets containing many
        readings.
2. Microservices + Configuration files (configurablity)
    - Microservices are commonly used to allow for parallel development of
        multiple pieces of a platform, without the strict limitations that come
        from a monolithic application. Each sevice is designed with some form of
        API (using REST, GraphQL, or some other communication protocol) that
        other services use to communicate with it.
    - Microservices also allow for potentially simpler configuration. While the
        overall platform configuration may be as dense as a monolithic
        application, the individual configurations may be more directed and
        spread out between multiple files.
3. Containerization (ease of setup)
    - Containerization allows for potentially very simple setup procedures by
        removing almost all "system level" setup requirements. Starting up the
        platform can be as simple as running a few scripts that automatically
        build all prerequisites and launch the full suite of containers.
    - There is also a very large security benefit to containerization. Each
        container runs independently inside a larger container (a pod). The pod
        only has access to the ports and files explicitly defined for it. A
        bad actor gaining access to a container is limited to just that
        container and its connected pods, but not the host system.
    - Containers also allow for some degree of Infrastructure-as-Code, or IaC.
        IaC allows for the applications, the configuration, and hardware
        required for a platform to be defined purely in code. This code
        (usually, a configuration file) can then be version controlled and
        easily extended for new uses. Tools like Kubernetes and Teraform are
        entirely designed around this paradigm. Containerization with Podman is
        only partially IaC due to a lack of multi-compute capabilities, but
        extending to Kubernetes would not be extremely difficult.

Goals 2 and 3 are inherently linked due to how containerization allows for
features like load-balancing and failover. Microservices generally make
implementing those features easier due to the simple nature of the individual
services. If each service is designed to do only one thing, then it may be
possible to stand up multiple of those services to allow for higher throughput
at peak demand, or for resiliency if any one service fails.

The architecture of the platform is summarized in the following diagram:

```
                    1. Sensors

                 ┌───┐ ┌───┐ ┌───┐
                 │   │ │   │ │   │
                 └───┘ └─┬─┘ └───┘
                         │
                         ▼
                ┌──────────────────┐
                │                  │
                │  2. MQTT Broker  │
                │                  │
                └────────┬─────────┘
                         │
                         ▼
                ┌──────────────────┐
                │                  │
                │  3. Ingest App.  │
                │                  │
                └─────┬────────────┘
                      │      ▲
                      ▼      │              ┌───────────────────┐
               ┌─────────────┴─────┐        │                   │
               │                   ├──────► │   5. Database     │
           ▲   │  4. Database API  │        │    (PostgreSQL)   │
           │   │                   │◄───────┤                   │
   Backend │   └──────┬────────────┘        └───────────────────┘
                      │      ▲
       ────────────── │ ──── │ ─────────────
                      ▼      │
  Frontend │    ┌────────────┴────┐
           │    │                 │
           ▼    │   6. Frontend   │
                │                 │
                └─────┬───────────┘
                      │      ▲
                      ▼      │
                 ┌───────────┴───┐
                 │               │
                 │    7.  UI     │
                 │               │
                 └───────────────┘
```

Each point is further explained below:

1. Sensors publish to an MQTT broker using a simple payload containing only a
   single raw floating point data value.
2. The MQTT broker acts as a transfer layer between the sensor and ingest
   application. The broker allows for new applications to hook onto the raw
   datastream without interferring with the pre-existing platform.
3. An ingest application takes in the raw feed of data from the MQTT broker.
    - The ingest application performs realtime analytics on sensors and is configured
        prior to startup for how it should handle each sensor type. For example,
        the ingest tool could treat a temperature sensor by performing a set of
        average, minimum, and maximum calculations every X seconds, while
        treating a door sensor by calculating the number of activations over
        the last X seconds. Currently, X is a global interval, not configurable
        per sensor type.
4. The ingest application submits calculations into the database API.
    - The database API is a REST API that allows for GET and POST of sensors
        and readings.
5. Results from the ingest application are subimtted to the database.
6. A front-end application allows for communication between the database and a
   user.
7. The UI allows users to define custom dashboards for their use-case, then share
   the embedded code to others and allow for them to see the same dashboard.

Both 6 and 7 are not implemented in this codebase as of right now (the
front-end and UI). Maybe later, if time allows.

## Limitations

Some aspects of this design may seem limited or unscalable. Examples may be the
linear structure without an event streamer like Kafka, or how the sensors have
no metadata in their postings, or the fairly simple database schema. In some
ways, all of this is on purpose. The goal of this project is to explore the
design aspects of a platform of this scope and to design what I can now. It is
not to build a fully feature-rich platform that will be a drop in for any
usecase, at least right now.

I plan on using this someday for IoT sensors around my home. I'm hoping to
build the "bones" of everything and fill in what I can now. Later, when I have
a full use for this project, I can go through and fill in the remainder, which
is hopefully a small amount, assuming I do a decent job now of planning for the
future.

## TODO

- Build out testing on each component.
    - [x] Add unit testing to `db-api`
    - [x] Refactor and add unit tests to `ingest`
    - [ ] Add integration testing
- [ ] Cleanup fake passwords and correctly use secrets. (There aren't any actual secrets in the codebase, but there are placeholder "passwords")
- [ ] Modify the sensor payload to take in a raw f32 instead of a encoded Rust struct.
- [ ] Potentially add metadata to the sensor payload. Not sure exactly what would be useful, but maybe.
- [ ] Add alerting to ingest's features. Send an email or a payload to a specific URL.
- [ ] Allow ingest to post readings without calculations (good for low frequency sensors).
- [ ] Switch to using yaml configs everywhere and remove environmental variables unless needed.
- [ ] Correctly handle errors around the codebase.
- [ ] Implement pagination and request throttling into `db-api`.

## License
MIT
