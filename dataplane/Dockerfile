FROM ubuntu:23.10 as builder

ARG TARGETARCH

RUN apt update
RUN apt install --yes build-essential protobuf-compiler curl pkg-config llvm
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y

ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup default stable
RUN rustup install nightly
RUN rustup component add rust-src --toolchain nightly
RUN --mount=type=cache,target=/root/.cargo/registry \
    cargo install bpf-linker

WORKDIR /workspace
# Docker uses the amd64/arm64 convention while Rust uses the x86_64/aarch64 convention.
# Since Dockerfile doesn't support conditional variables (sigh), write the arch in Rust's
# convention to a file for later usage.
RUN if [ "$TARGETARCH" = "amd64" ]; \
    then echo "x86_64" >> arch; \
    else echo "aarch64" >> arch; \
    fi
RUN rustup target add $(eval cat arch)-unknown-linux-musl

COPY . .

# We need to tell bpf-linker where it can find LLVM's shared library file.
# Ref: https://github.com/aya-rs/rustc-llvm-proxy/blob/cbcb3c6/src/lib.rs#L48
ENV LD_LIBRARY_PATH="/usr/lib/llvm-16/lib"

RUN --mount=type=cache,target=/workspace/target/ \
    --mount=type=cache,target=/root/.cargo/registry \
    cargo xtask build-ebpf --release
RUN --mount=type=cache,target=/workspace/target/ \
    --mount=type=cache,target=/root/.cargo/registry \
    RUSTFLAGS=-Ctarget-feature=+crt-static cargo build --release --target=$(eval cat arch)-unknown-linux-musl
RUN --mount=type=cache,target=/workspace/target/ \
    cp /workspace/target/$(eval cat arch)-unknown-linux-musl/release/loader /workspace/dataplane

FROM alpine

LABEL org.opencontainers.image.source=https://github.com/kubernetes-sigs/blixt
LABEL org.opencontainers.image.licenses=GPL-2.0-only,BSD-2-Clause

WORKDIR /opt/blixt/

COPY --from=builder /workspace/dataplane /opt/blixt/dataplane

COPY LICENSE.GPL-2.0 /opt/blixt/LICENSE.GPL-2.0
COPY LICENSE.BSD-2-Clause /opt/blixt/LICENSE.BSD-2-Clause

ENTRYPOINT ["/opt/blixt/dataplane"]
