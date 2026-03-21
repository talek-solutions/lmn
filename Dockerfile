FROM rust:slim AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY lmn-core ./lmn-core
COPY lmn-cli ./lmn-cli

RUN cargo build --release --locked -p lmn

FROM debian:bookworm-slim AS runtime

COPY --from=builder /app/target/release/lmn /usr/local/bin/lmn

ENTRYPOINT ["lmn"]
