FROM rust:1.92-slim AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY .cargo/config.toml /usr/local/cargo/config.toml
COPY .cargo/credentials.toml /usr/local/cargo/credentials.toml

COPY Cargo.toml Cargo.lock ./
COPY herald-core/ herald-core/
COPY herald-server/ herald-server/

RUN cargo build --release --bin herald

FROM debian:trixie-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd -r -s /bin/false herald && \
    mkdir -p /data && chown herald:herald /data

COPY --from=builder /build/target/release/herald /usr/local/bin/herald
COPY docker-entrypoint.sh /docker-entrypoint.sh
RUN chmod +x /docker-entrypoint.sh

VOLUME /data
WORKDIR /data

# Single port: HTTP API + WebSocket (/ws)
EXPOSE 6200

ENTRYPOINT ["/docker-entrypoint.sh"]
CMD ["herald"]
