FROM rust as builder

RUN apt-get update && \
    apt-get install musl-tools -yq && \
    rustup target add x86_64-unknown-linux-musl

WORKDIR /workspace

COPY Cargo.toml Cargo.lock ./
COPY dataplane dataplane
COPY tools/udp-test-server/Cargo.toml tools/udp-test-server/Cargo.toml
COPY tools/udp-test-server/Cargo.lock tools/udp-test-server/Cargo.lock
COPY tools/udp-test-server/src/main.rs tools/udp-test-server/src/main.rs
COPY xtask xtask

RUN --mount=type=cache,target=/workspace/target/ \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
    RUSTFLAGS=-Ctarget-feature=+crt-static cargo build \
    --package=udp-test-server \
    --release \
    --target=x86_64-unknown-linux-musl

RUN --mount=type=cache,target=/workspace/target/ \
    cp /workspace/target/x86_64-unknown-linux-musl/release/udp-test-server /workspace/udp-test-server

FROM alpine

LABEL org.opencontainers.image.source https://github.com/kubernetes-sigs/blixt

WORKDIR /

COPY --from=builder /workspace/udp-test-server /udp-test-server

ENTRYPOINT ["/udp-test-server"]
