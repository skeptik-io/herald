FROM rust:1.92-slim AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Copy cargo config for ShroudB registry (injected by CI or local .cargo/)
COPY .cargo/config.toml /usr/local/cargo/config.toml
COPY .cargo/credentials.toml /usr/local/cargo/credentials.toml

# Copy source
COPY Cargo.toml Cargo.lock ./
COPY herald-core/ herald-core/
COPY herald-server/ herald-server/

RUN cargo build --release --bin herald

FROM debian:trixie-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd -r -s /bin/false herald

COPY --from=builder /build/target/release/herald /usr/local/bin/herald

USER herald
WORKDIR /data

EXPOSE 6200 6201

ENTRYPOINT ["herald"]
CMD ["herald.toml"]
