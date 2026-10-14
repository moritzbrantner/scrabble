FROM rust:1.99.0-bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6 AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
RUN cargo build -p scrabble-server --release --locked

FROM debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587
RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /build/target/release/scrabble-server /usr/local/bin/scrabble-server
USER 1000:1000
EXPOSE 4433/udp 8080/tcp 8081/tcp
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=10s --timeout=2s --start-period=20s --retries=3 \
    CMD curl --fail --silent --max-time 2 http://127.0.0.1:8080/readyz > /dev/null || exit 1
ENTRYPOINT ["scrabble-server"]
