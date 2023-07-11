# IoT Dashboard

IoT Dashboard is a proof-of-concept project of a "full" platform for an IoT
ingest and dashboarding system. IoT in this case is defined as a set of small
internet-connected sensors that transmit small packets at a high frequency,
almost immediately after data acquisition. This platform starts by taking in raw
data from sensors over some transport layer (MQTT), then performing real-time
analytics and submitting to a database (PostgreSQL). A front-end for custom
visualization is also available (Django + React).

All of this is designed to be containerized and can run semi-independently. Some
requirements exist, like the connection between the real-time analytics
component and the database, or the front-end and the database. All container
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
    - The dashboard would be used by non-engineering (business, finance, etc)
        members for reporting purposes.


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

Monitor the pod's startup:

```bash
podman ps --pod
```

Pod resource usage can be found with:

```bash
podman pod stats
```

## Architecture

The three goals for development of this platform, as well as the rationale behind
them, are:

1. Low latency
    - Faults on a production line or a garage door being open at an unexpected
        time require quick alerting to prevent downstream issues from occurring.
        An ingest application allows for configuration of the analytics and
        alerting from realtime data.
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
                ▼      │
         ┌─────────────┴─────┐
         │                   │
         │   4. Database     │
         │    (PostgreSQL)   │   ▲
         │                   │   │
         └──────┬────────────┘   │ Backend
                │      ▲
 ────────────── │ ──── │ ─────────────
                ▼      │
          ┌────────────┴───┐     │ Frontend
          │                │     │
          │  5. Frontend   │     ▼
          │     (Django)   │
          │                │
          └────┬───────────┘
               │      ▲
               ▼      │
           ┌──────────┴───┐
           │              │
           │  6.  UI      │
           │    (React)   │
           │              │
           └──────────────┘
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
        treating a door sensor by calculating the number of activations over the
        last X seconds.
4. Results from the ingest application are subimtted to the database.
5. A front-end application allows for communication between the database and a
   user.
6. The UI allows users to define custom dashboards for their use-case, then share
   the embedded code to others and allow for them to see the same dashboard.
