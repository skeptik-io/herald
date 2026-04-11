FROM rust:1.92-slim AS builder

ARG HERALD_FEATURES="chat,presence"

WORKDIR /build

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY .cargo/config.toml /usr/local/cargo/config.toml
COPY .cargo/credentials.toml /usr/local/cargo/credentials.toml

COPY Cargo.toml Cargo.lock ./
COPY herald-core/ herald-core/
COPY herald-server/ herald-server/

RUN if [ -z "$HERALD_FEATURES" ]; then \
      cargo build --release --bin herald --no-default-features; \
    else \
      cargo build --release --bin herald --no-default-features --features "$HERALD_FEATURES"; \
    fi

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
