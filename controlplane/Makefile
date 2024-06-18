IMAGE ?= ghcr.io/kubernetes-sigs/blixt-controlplane-rs
TAG ?= latest
KIND_CLUSTER ?= blixt-dev

all: build

.PHONY:
clean:
	cargo clean

.PHONY: build
build:
	cargo build

.PHONY: build.release
build.release:
	cargo build --release

.PHONY: run
run:
	cargo run

.PHONY: build.image
build.image:
	DOCKER_BUILDKIT=1 docker build -t $(IMAGE):$(TAG) ./
