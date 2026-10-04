FROM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
RUN cargo build -p scrabble-server --release --locked

FROM debian:bookworm-slim@sha256:3783cc01769c7b2b1b83a5c5ad96c815348e28ed7da68e2e3687004faa906251
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
