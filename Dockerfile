FROM rust:alpine AS builder

WORKDIR /app

RUN apk add --no-cache musl-dev

COPY Cargo.toml Cargo.lock ./
COPY lmn-core ./lmn-core
COPY lmn-cli ./lmn-cli

RUN cargo build --release --locked -p lmn

FROM alpine:3 AS runtime

COPY --from=builder /app/target/release/lmn /usr/local/bin/lmn

EXPOSE 3001

ENTRYPOINT ["lmn"]
