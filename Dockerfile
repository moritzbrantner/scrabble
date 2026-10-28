FROM rust:1.99.1-bookworm@sha256:e9da6f6c3ed5095fd5d33bf7cdf309830a178a8147888f1b2f76d5769bd9306c AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
RUN cargo build -p scrabble-server --release --locked

FROM debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3
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
