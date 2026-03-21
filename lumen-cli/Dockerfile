FROM rust:slim AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY lumen-core ./lumen-core
COPY lumen-cli ./lumen-cli

RUN cargo build --release -p lmn

FROM debian:bookworm-slim AS runtime

COPY --from=builder /app/target/release/lmn /usr/local/bin/lmn

ENTRYPOINT ["lmn"]
