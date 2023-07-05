FROM rust:1.70 as builder
WORKDIR /usr/src/inlet
COPY . .
RUN cargo install --path .

From debian:bullseye-slim
# UN apt-get update && apt-get install -y extra-runtime-dependencies && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/inlet /usr/local/bin/inlet
CMD ["inlet"]
