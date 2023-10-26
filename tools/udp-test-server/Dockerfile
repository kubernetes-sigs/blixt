FROM rust as builder

RUN apt-get update
RUN apt-get install musl-tools -yq

RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /workspace

COPY Cargo.toml Cargo.toml
COPY Cargo.lock Cargo.lock
COPY src/main.rs src/main.rs

RUN --mount=type=cache,target=/workspace/target/ \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
    RUSTFLAGS=-Ctarget-feature=+crt-static cargo build --release --target=x86_64-unknown-linux-musl

RUN --mount=type=cache,target=/workspace/target/ \
    cp /workspace/target/x86_64-unknown-linux-musl/release/udp-test-server /workspace/udp-test-server

FROM alpine

LABEL org.opencontainers.image.source https://github.com/kubernetes-sigs/blixt

WORKDIR /

COPY --from=builder /workspace/udp-test-server /udp-test-server

ENTRYPOINT ["/udp-test-server"]
