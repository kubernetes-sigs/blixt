IMAGE ?= ghcr.io/kong/blixt-dataplane
TAG ?= latest
KIND_CLUSTER ?= blixt-dev

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
	cargo xtask build-ebpf --release
	cargo build --release

.PHONY: build.image
build.image:
	DOCKER_BUILDKIT=1 docker build -t $(IMAGE):$(TAG) ./

.PHONY: load.image
load.image: build.image
	kind load docker-image $(IMAGE):$(TAG) --name $(KIND_CLUSTER) && \
		kubectl -n blixt-system rollout restart daemonset blixt-dataplane
