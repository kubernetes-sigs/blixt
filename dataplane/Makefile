IMAGE ?= ghcr.io/kubernetes-sigs/blixt-dataplane
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

.PHONY: build.bytecode.images
build.bytecode.images: build
	docker build \
      --build-arg PROGRAM_NAME=blixt-tc-ingress \
      --build-arg BPF_FUNCTION_NAME=tc_ingress \
      --build-arg PROGRAM_TYPE=tc \
      --build-arg BYTECODE_FILENAME=loader \
      -f https://raw.githubusercontent.com/bpfd-dev/bpfd/main/packaging/container-deployment/Containerfile.bytecode \
	  ./target/bpfel-unknown-none/debug -t quay.io/bpfd-bytecode/blixt-tc-ingress:latest
	docker build \
      --build-arg PROGRAM_NAME=blixt-tc-egress \
      --build-arg BPF_FUNCTION_NAME=tc_egress \
      --build-arg PROGRAM_TYPE=tc \
      --build-arg BYTECODE_FILENAME=loader \
      -f https://raw.githubusercontent.com/bpfd-dev/bpfd/main/packaging/container-deployment/Containerfile.bytecode \
	  ./target/bpfel-unknown-none/debug -t quay.io/bpfd-bytecode/blixt-tc-egress:latest

.PHONY: push.bytecode.images
push.bytecode.images: build.bytecode.images
	docker push quay.io/bpfd-bytecode/blixt-tc-egress:latest
	docker push quay.io/bpfd-bytecode/blixt-tc-ingress:latest
