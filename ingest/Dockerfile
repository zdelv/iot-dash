FROM rust:1.70 as builder
WORKDIR /usr/src/ingest
RUN mkdir ingest
RUN mkdir ingest/src

COPY ./ingest/.env .env
COPY ./ingest/src ./ingest/src
COPY ./ingest/Cargo* ./ingest/.

COPY ./common ./common

RUN cargo install --path ingest

From debian:bullseye-slim
# UN apt-get update && apt-get install -y extra-runtime-dependencies && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/ingest /usr/local/bin/ingest
COPY --from=builder /usr/src/ingest/.env /.env
