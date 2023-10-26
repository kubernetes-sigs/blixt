IMAGE ?= ghcr.io/kubernetes-sigs/blixt-perf-test-server
TAG ?= latest

all: build.image

.PHONY: build.image
build.image:
	docker build -t $(IMAGE):$(TAG) ./

.PHONY: push.image
push.image: build.image
	docker push $(IMAGE):$(TAG)
