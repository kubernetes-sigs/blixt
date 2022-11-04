FROM rust as builder

WORKDIR /workspace

COPY . .

RUN rustup install nightly
RUN rustup component add rust-src --toolchain nightly
RUN cargo install bpf-linker

RUN cargo xtask build-ebpf
RUN cargo build

FROM archlinux

LABEL org.opencontainers.image.source=https://github.com/kong/blixt
LABEL org.opencontainers.image.licenses=GPL-2.0-only

WORKDIR /

RUN pacman -Sy

RUN pacman -S bpf ethtool xdp-tools bpftrace --noconfirm

COPY --from=builder /workspace/target/debug/loader /blixt-dataplane
COPY LICENSE /workspace/LICENSE

ENTRYPOINT ["/blixt-dataplane"]
