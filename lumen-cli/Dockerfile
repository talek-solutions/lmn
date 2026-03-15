FROM rust:slim AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY lumen-core ./lumen-core
COPY lumen-cli ./lumen-cli

RUN cargo build --release -p lumen

FROM debian:bookworm-slim AS runtime

COPY --from=builder /app/target/release/lumen /usr/local/bin/lumen

ENTRYPOINT ["lumen"]
