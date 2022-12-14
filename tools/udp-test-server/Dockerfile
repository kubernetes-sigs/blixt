FROM rust as builder

RUN apt-get update

RUN apt-get install musl-tools -yq

RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /workspace

COPY Cargo.toml Cargo.toml

COPY Cargo.lock Cargo.lock

COPY src/main.rs src/main.rs

RUN RUSTFLAGS=-Ctarget-feature=+crt-static cargo build --release --target=x86_64-unknown-linux-musl

FROM gcr.io/distroless/static

LABEL org.opencontainers.image.source https://github.com/kong/blixt-udp-test-server

WORKDIR /

COPY --from=builder /workspace/target/x86_64-unknown-linux-musl/release/udp-test-server /udp-test-server

ENTRYPOINT ["/udp-test-server"]
