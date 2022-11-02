FROM archlinux as builder

LABEL org.opencontainers.image.source=https://github.com/kong/blixt
LABEL org.opencontainers.image.licenses=GPL-2.0-only

RUN pacman -Syu --noconfirm
RUN pacman -S base-devel bpf git make go clang llvm linux-headers --noconfirm

WORKDIR /workspace

RUN git clone https://github.com/libbpf/libbpf.git

COPY go.mod go.mod
COPY go.sum go.sum

RUN go mod download

COPY main.go main.go
COPY xdploader.go xdploader.go
COPY router.go router.go
COPY utils.go utils.go
COPY udproute_controller.go udproute_controller.go
COPY pod_controller.go pod_controller.go
COPY xdp.c xdp.c
COPY Makefile Makefile

RUN CFLAGS="-I/usr/include/x86_64-linux-gnu/" make

FROM archlinux

WORKDIR /

RUN pacman -Sy

RUN pacman -S bpf ethtool xdp-tools bpftrace --noconfirm

COPY --from=builder /workspace/blixt-dataplane .
COPY LICENSE /workspace/LICENSE

ENTRYPOINT ["/blixt-dataplane"]
