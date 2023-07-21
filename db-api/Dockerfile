FROM rust:1.70 as builder
WORKDIR /usr/src/db-api
RUN mkdir db-api
RUN mkdir db-api/src

COPY ./db-api/.env .env
COPY ./db-api/src ./db-api/src
COPY ./db-api/Cargo* ./db-api/.

COPY ./common ./common

RUN cargo install --path db-api

From debian:bullseye-slim
# UN apt-get update && apt-get install -y extra-runtime-dependencies && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/db-api /usr/local/bin/db-api
COPY --from=builder /usr/src/db-api/.env /.env
