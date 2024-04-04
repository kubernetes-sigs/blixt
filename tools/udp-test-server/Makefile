IMAGE ?= ghcr.io/kubernetes-sigs/blixt-udp-test-server
TAG ?= latest

.PHONY:
clean:
	cargo clean

.PHONY: build
build:
	cargo build

.PHONY: build.image
build.image:
	 DOCKER_BUILDKIT=1 docker build -t $(IMAGE):$(TAG) ./

.PHONY: push.image
push.image: build.image
	docker push $(IMAGE):$(TAG)
