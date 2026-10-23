FROM rust:1.99.1-bookworm@sha256:48b7f435b8fc6378ce43b44f15e3a47fd8919b2825f835c39bd4bd3da50af5b8 AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
RUN cargo build -p scrabble-server --release --locked

FROM debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8
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
