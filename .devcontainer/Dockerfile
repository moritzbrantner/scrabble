# Prebuilt agent environment candidate for #81. The Playwright base matches
# @playwright/test 1.63.0 and already contains Chromium with its system libraries.
# Bun comes from package.json `packageManager` and Rust from rust-toolchain.toml,
# read at build time, so this file adds no second version authority.
FROM mcr.microsoft.com/playwright:v1.64.1-noble@sha256:7852dc5a7d72f5fe458a597d690f6c42fbc8e9e26ddfa95f8e6ef552558d9bbf

RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential ca-certificates curl git jq pkg-config unzip \
    && rm -rf /var/lib/apt/lists/*

COPY package.json /tmp/bun-version/package.json
RUN version="$(jq -er '.packageManager | capture("^bun@(?<v>[0-9.]+)$").v' /tmp/bun-version/package.json)" \
    && base="https://github.com/oven-sh/bun/releases/download/bun-v${version}" \
    && cd /tmp/bun-version \
    && curl -fsSLO "${base}/bun-linux-x64.zip" \
    && curl -fsSL "${base}/SHASUMS256.txt" | grep ' bun-linux-x64.zip$' | sha256sum -c - \
    && unzip -q bun-linux-x64.zip \
    && install -m755 bun-linux-x64/bun /usr/local/bin/bun \
    && ln -s bun /usr/local/bin/bunx \
    && cd / && rm -rf /tmp/bun-version

ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
COPY rust-toolchain.toml /tmp/toolchain/rust-toolchain.toml
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal --default-toolchain none \
    && cd /tmp/toolchain && rustup toolchain install \
    && rustup default "$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)" \
    && rm -rf /tmp/toolchain
