IMAGE ?= ghcr.io/kong/blixt-udp-test-server
TAG ?= latest

.PHONY: build.image
build.image:
	docker build -t $(IMAGE):$(TAG) ./

.PHONY: push.image
push.image: build.image
	docker push $(IMAGE):$(TAG)
