IMAGE ?= ghcr.io/kong/blixt-dataplane
TAG ?= latest

all: build

.PHONY:
clean:
	cargo clean

.PHONY: build
build:
	cargo xtask build-ebpf
	cargo build

.PHONY: build.release
build.release:
	cargo xtask build-ebpf
	cargo build --release

.PHONY: build.image
build.image:
	DOCKER_BUILDKIT=1 docker build -t $(IMAGE):$(TAG) ./
