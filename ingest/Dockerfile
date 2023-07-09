FROM rust:1.70 as builder
WORKDIR /usr/src/ingest
RUN mkdir src

COPY ./.env .env
COPY ./src ./src
COPY ./Cargo* .

RUN cargo install --path .

From debian:bullseye-slim
# UN apt-get update && apt-get install -y extra-runtime-dependencies && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/ingest /usr/local/bin/ingest
COPY --from=builder /usr/src/ingest/.env /.env
