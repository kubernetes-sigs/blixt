# ------------------------------------------------------------------------------
# Build Dependencies
# ------------------------------------------------------------------------------

## Location to install dependencies to
LOCALBIN ?= $(shell pwd)/build/bin
$(LOCALBIN):
	mkdir -p $(LOCALBIN)

## Tool Binaries
CFSSL ?= $(LOCALBIN)/cfssl
CFSSLJSON ?= $(LOCALBIN)/cfssljson
ENVTEST ?= $(LOCALBIN)/setup-envtest
KIND ?= $(LOCALBIN)/kind
CLOUD_PROVIDER_KIND ?= $(LOCALBIN)/cloud-provider-kind

## Tool Versions
CFSSL_VERSION ?= v1.6.5
KUSTOMIZE_VERSION ?= v5.3.0
KIND_VERSION ?= v0.29.0
CLOUD_PROVIDER_KIND_VERSION ?= v0.6.0

# CFSSL config
TEST_CERTS_PATH ?= config/tests/auth/certs

.PHONY: cfssl
cfssl: $(CFSSL) ## Download cfssl locally if necessary
$(CFSSL): $(LOCALBIN)
	test -s $(LOCALBIN)/cfssl ||  GOBIN=$(LOCALBIN) go install github.com/cloudflare/cfssl/cmd/cfssl@$(CFSSL_VERSION)

.PHONY: cfssljson
cfssljson: $(CFSSLJSON)
$(CFSSLJSON): $(LOCALBIN)
	test -s $(LOCALBIN)/cfssljson || GOBIN=$(LOCALBIN) go install github.com/cloudflare/cfssl/cmd/cfssljson@$(CFSSL_VERSION)

.PHONY: cloud-provider-kind
cloud-provider-kind: $(CLOUD_PROVIDER_KIND)
$(CLOUD_PROVIDER_KIND): $(LOCALBIN)
	GOBIN=$(LOCALBIN) go install sigs.k8s.io/cloud-provider-kind@$(CLOUD_PROVIDER_KIND_VERSION)

.PHONY: kind
kind: $(CLOUD_PROVIDER_KIND) $(KIND)
$(KIND): $(LOCALBIN)
	GOBIN=$(LOCALBIN) go install sigs.k8s.io/kind@$(KIND_VERSION)

# ------------------------------------------------------------------------------
# Build
# ------------------------------------------------------------------------------

.PHONY: all
all: build

.PHONY: help
help: ## Display this help.
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n"} /^[a-zA-Z_0-9-]+:.*?##/ { printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)

.PHONY: clean
clean: ## clean repo
	cargo clean
	rm $(TEST_CERTS_PATH)/{*.pem,*.csr}

.PHONY: build
build: ## Build dataplane
	cargo xtask build-ebpf
	cargo build

.PHONY: build.release
build.release: ## Build dataplane release
	cargo xtask build-ebpf --release
	cargo build --release

# ------------------------------------------------------------------------------
# Build Images
# ------------------------------------------------------------------------------

# Container Runtimes
CONTAINER_RUNTIME ?= podman

# Container Images
REGISTRY ?= ghcr.io/kubernetes-sigs
BLIXT_CONTROLPLANE_IMAGE ?= $(REGISTRY)/blixt-controlplane
BLIXT_DATAPLANE_IMAGE ?= $(REGISTRY)/blixt-dataplane
BLIXT_UDP_SERVER_IMAGE ?= $(REGISTRY)/blixt-udp-test-server
TAG ?= integration-tests

# Containerfile paths for each service
CONTROLPLANE_CONTAINERFILE ?= build/Containerfile.controlplane
DATAPLANE_CONTAINERFILE ?= build/Containerfile.dataplane
UDP_SERVER_CONTAINERFILE ?= build/Containerfile.udp_test_server

.PHONY: build.image.controlplane
build.image.controlplane:
	$(CONTAINER_RUNTIME) build $(BUILD_ARGS) --file=$(CONTROLPLANE_CONTAINERFILE) -t $(BLIXT_CONTROLPLANE_IMAGE):$(TAG) ./

.PHONY: build.image.udp_test_server
build.image.udp_test_server:
	$(CONTAINER_RUNTIME) build $(BUILD_ARGS) --file=$(UDP_SERVER_CONTAINERFILE) -t $(BLIXT_UDP_SERVER_IMAGE):$(TAG) ./

.PHONY: build.image.dataplane
build.image.dataplane:
	$(CONTAINER_RUNTIME) build $(BUILD_ARGS) --file=$(DATAPLANE_CONTAINERFILE) -t $(BLIXT_DATAPLANE_IMAGE):$(TAG) ./

.PHONY: build.all.images
build.all.images: 
	$(MAKE) build.image.controlplane
	$(MAKE) build.image.dataplane
	$(MAKE) build.image.udp_test_server

# ------------------------------------------------------------------------------
# Development
# ------------------------------------------------------------------------------

.PHONY: fix.format
fix.format.rust: ## Autofix Rust code formatting
	cargo fmt --manifest-path Cargo.toml --all

.PHONY: check.format
check.format.rust: ## Check Rust code formatting
	cargo fmt --manifest-path Cargo.toml --all -- --check

.PHONY: lint
lint:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

# ------------------------------------------------------------------------------
# Testing
# ------------------------------------------------------------------------------

.PHONY: test
test:
	cargo test -vv --workspace --exclude tests-integration

.PHONY: test.gencert
test.gencert: cfssl cfssljson
	$(CFSSL) gencert \
		-initca $(TEST_CERTS_PATH)/ca-csr.json | $(CFSSLJSON) -bare $(TEST_CERTS_PATH)/ca
	$(CFSSL) gencert \
		-ca=$(TEST_CERTS_PATH)/ca.pem \
		-ca-key=$(TEST_CERTS_PATH)/ca-key.pem \
		-config=$(TEST_CERTS_PATH)/ca-config.json \
		-profile=server \
		$(TEST_CERTS_PATH)/server-csr.json | $(CFSSLJSON) -bare $(TEST_CERTS_PATH)/server
	$(CFSSL) gencert \
		-ca=$(TEST_CERTS_PATH)/ca.pem \
		-ca-key=$(TEST_CERTS_PATH)/ca-key.pem \
		-config=$(TEST_CERTS_PATH)/ca-config.json \
		-profile=clinet \
		$(TEST_CERTS_PATH)/client-csr.json | $(CFSSLJSON) -bare $(TEST_CERTS_PATH)/client

.PHONY: test.rmcert
test.rmcert:
	rm $(TEST_CERTS_PATH)/{*.pem,*.csr}

# ------------------------------------------------------------------------------
# Deployment
# ------------------------------------------------------------------------------

KIND_CLUSTER ?= blixt-dev

.PHONY: install
install: manifests
	kubectl kustomize config/crd | kubectl apply -f -

.PHONY: uninstall
uninstall: manifests
	kubectl kustomize config/crd | kubectl delete --ignore-not-found=$(ignore-not-found) -f -

.PHONY: deploy
deploy: manifests
	kubectl kustomize config/default | kubectl apply -f -

.PHONY: undeploy
undeploy:
	kubectl kustomize config/default | kubectl delete --ignore-not-found=$(ignore-not-found) -f -

.PHONY: build.cluster
build.cluster: $(KIND)
	$(KIND) create cluster --name $(KIND_CLUSTER)
	echo "use $(CLOUD_PROVIDER_KIND) to enable LoadBalancer type Services"

.PHONY: load.image.controlplane
load.image.controlplane: build.image.controlplane
	kubectl create namespace blixt-system || true && \
	kind load docker-image $(BLIXT_CONTROLPLANE_IMAGE):$(TAG) --name $(KIND_CLUSTER) && \
		kubectl -n blixt-system get deployment blixt-controlplane >/dev/null 2>&1 && \
		kubectl -n blixt-system rollout restart deployment blixt-controlplane || true

.PHONY: load.image.dataplane
load.image.dataplane: build.image.dataplane
	kubectl create namespace blixt-system || true && \
	kind load docker-image $(BLIXT_DATAPLANE_IMAGE):$(TAG) --name $(KIND_CLUSTER) && \
		kubectl -n blixt-system rollout restart daemonset dataplane || true

.PHONY: load.all.images
load.all.images: build.all.images
	kind load docker-image $(BLIXT_CONTROLPLANE_IMAGE):$(TAG) --name $(KIND_CLUSTER) && \
	kind load docker-image $(BLIXT_DATAPLANE_IMAGE):$(TAG) --name $(KIND_CLUSTER) && \
	kind load docker-image $(BLIXT_UDP_SERVER_IMAGE):$(TAG) --name $(KIND_CLUSTER) && \
		kubectl -n blixt-system get deployment blixt-controlplane >/dev/null 2>&1 && \
		kubectl -n blixt-system rollout restart deployment blixt-controlplane || true
