# ------------------------------------------------------------------------------
# Build Variables
# ------------------------------------------------------------------------------

# IMAGES used when running tests.
BLIXT_CONTROLPLANE_IMAGE ?= ghcr.io/kubernetes-sigs/blixt-controlplane
BLIXT_DATAPLANE_IMAGE ?= ghcr.io/kubernetes-sigs/blixt-dataplane
BLIXT_UDP_SERVER_IMAGE ?= ghcr.io/kubernetes-sigs/blixt-udp-test-server

# Dockerfile paths for each service
CONTROLPLANE_DOCKERFILE ?= build/Containerfile.controlplane
DATAPLANE_DOCKERFILE ?= build/Containerfile.dataplane
UDP_SERVER_DOCKERFILE ?= build/Containerfile.udp_server

# Other testing variables
EXISTING_CLUSTER ?=

# Image URL to use all building/pushing image targets
TAG ?= integration-tests
ifeq ($(shell uname -m),arm64)
BUILD_PLATFORMS ?= linux/arm64
else
BUILD_PLATFORMS ?= linux/amd64
endif
BUILD_ARGS ?= --load

# VERSION defines the project version for the bundle.
# Update this value when you upgrade the version of your project.
# To re-generate a bundle for another specific version without changing the standard setup, you can:
# - use the VERSION as arg of the bundle target (e.g make bundle VERSION=0.0.2)
# - use environment variables to overwrite this value (e.g export VERSION=0.0.2)
VERSION ?= 0.0.1

# CHANNELS define the bundle channels used in the bundle.
# Add a new line here if you would like to change its default config. (E.g CHANNELS = "candidate,fast,stable")
# To re-generate a bundle for other specific channels without changing the standard setup, you can:
# - use the CHANNELS as arg of the bundle target (e.g make bundle CHANNELS=candidate,fast,stable)
# - use environment variables to overwrite this value (e.g export CHANNELS="candidate,fast,stable")
ifneq ($(origin CHANNELS), undefined)
BUNDLE_CHANNELS := --channels=$(CHANNELS)
endif

# DEFAULT_CHANNEL defines the default channel used in the bundle.
# Add a new line here if you would like to change its default config. (E.g DEFAULT_CHANNEL = "stable")
# To re-generate a bundle for any other default channel without changing the default setup, you can:
# - use the DEFAULT_CHANNEL as arg of the bundle target (e.g make bundle DEFAULT_CHANNEL=stable)
# - use environment variables to overwrite this value (e.g export DEFAULT_CHANNEL="stable")
ifneq ($(origin DEFAULT_CHANNEL), undefined)
BUNDLE_DEFAULT_CHANNEL := --default-channel=$(DEFAULT_CHANNEL)
endif
BUNDLE_METADATA_OPTS ?= $(BUNDLE_CHANNELS) $(BUNDLE_DEFAULT_CHANNEL)

# IMAGE_TAG_BASE defines the docker.io namespace and part of the image name for remote images.
# This variable is used to construct full image tags for bundle and catalog images.
#
# For example, running 'make bundle-build bundle-push catalog-build catalog-push' will build and push both
# blixt.gateway.networking.k8s.io/blixt-bundle:$VERSION and blixt.gateway.networking.k8s.io/blixt-catalog:$VERSION.
IMAGE_TAG_BASE ?= blixt.gateway.networking.k8s.io/blixt

# BUNDLE_IMG defines the image:tag used for the bundle.
# You can use it as an arg. (E.g make bundle-build BUNDLE_IMG=<some-registry>/<project-name-bundle>:<tag>)
BUNDLE_IMG ?= $(IMAGE_TAG_BASE)-bundle:v$(VERSION)

# BUNDLE_GEN_FLAGS are the flags passed to the operator-sdk generate bundle command
BUNDLE_GEN_FLAGS ?= -q --overwrite --version $(VERSION) $(BUNDLE_METADATA_OPTS)

# USE_IMAGE_DIGESTS defines if images are resolved via tags or digests
# You can enable this value if you would like to use SHA Based Digests
# To enable set flag to true
USE_IMAGE_DIGESTS ?= false
ifeq ($(USE_IMAGE_DIGESTS), true)
	BUNDLE_GEN_FLAGS += --use-image-digests
endif

# ENVTEST_K8S_VERSION refers to the version of kubebuilder assets to be downloaded by envtest binary.
ENVTEST_K8S_VERSION = 1.24.2

# Get the currently used golang install path (in GOPATH/bin, unless GOBIN is set)
ifeq (,$(shell go env GOBIN))
GOBIN=$(shell go env GOPATH)/bin
else
GOBIN=$(shell go env GOBIN)
endif

# Setting SHELL to bash allows bash commands to be executed by recipes.
# Options are set to exit when a recipe line exits non-zero or a piped command fails.
SHELL = /usr/bin/env bash -o pipefail
.SHELLFLAGS = -ec

# Ensure missing resources are not skipped when applying changes (by default)
ifndef ignore-not-found
  ignore-not-found = false
endif

## Location to install dependencies to
LOCALBIN ?= $(shell pwd)/build/bin
$(LOCALBIN):
	mkdir -p $(LOCALBIN)

## Tool Binaries
KUSTOMIZE ?= $(LOCALBIN)/kustomize
ENVTEST ?= $(LOCALBIN)/setup-envtest
KIND ?= $(LOCALBIN)/kind
KTF ?= $(LOCALBIN)/ktf

## Tool Versions
KUSTOMIZE_VERSION ?= v5.3.0
CONTROLLER_TOOLS_VERSION ?= v0.14.0
KIND_VERSION ?= v0.22.0

KUSTOMIZE_INSTALL_SCRIPT ?= "https://raw.githubusercontent.com/kubernetes-sigs/kustomize/master/hack/install_kustomize.sh"

# ------------------------------------------------------------------------------
# Build Dependencies
# ------------------------------------------------------------------------------

.PHONY: kustomize
kustomize: $(KUSTOMIZE) ## Download kustomize locally if necessary.
$(KUSTOMIZE): $(LOCALBIN)
	test -s $(LOCALBIN)/kustomize || { curl -s $(KUSTOMIZE_INSTALL_SCRIPT) | bash -s -- $(subst v,,$(KUSTOMIZE_VERSION)) $(LOCALBIN); }

.PHONY: kind
kind: $(KIND)
$(KIND): $(LOCALBIN)
	test -s $(LOCALBIN)/setup-envtest || GOBIN=$(LOCALBIN) go install sigs.k8s.io/kind@$(KIND_VERSION)

.PHONY: ktf
ktf: $(KTF) $(KIND)
$(KTF): $(LOCALBIN)
	test -s $(LOCALBIN)/ktf || GOBIN=$(LOCALBIN) go install github.com/kong/kubernetes-testing-framework/cmd/ktf@latest

.PHONY: bundle
bundle: manifests kustomize ## Generate bundle manifests and metadata, then validate generated files.
	operator-sdk generate kustomize manifests -q
	cd config/manager && $(KUSTOMIZE) edit set image controller=$(IMG)
	$(KUSTOMIZE) build config/manifests | operator-sdk generate bundle $(BUNDLE_GEN_FLAGS)
	operator-sdk bundle validate ./bundle

.PHONY: bundle-build
bundle-build: ## Build the bundle image.
	DOCKER_BUILDKIT=1 docker build -f bundle.Dockerfile -t $(BUNDLE_IMG) .

.PHONY: bundle-push
bundle-push: ## Push the bundle image.
	$(MAKE) docker-push IMG=$(BUNDLE_IMG)

# ------------------------------------------------------------------------------
# Build
# ------------------------------------------------------------------------------

.PHONY: all
all: build

.PHONY: help
help: ## Display this help.
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n"} /^[a-zA-Z_0-9-]+:.*?##/ { printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)

.PHONY: clean
clean: ## Cargo clean
	cargo clean

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

.PHONY: build.image.controlplane
build.image.controlplane:
	DOCKER_BUILDKIT=1 docker buildx build --platform=$(BUILD_PLATFORMS) --file=$(CONTROLPLANE_DOCKERFILE) $(BUILD_ARGS) -t $(BLIXT_CONTROLPLANE_IMAGE):$(TAG) .

.PHONY: build.image.udp_server
build.image.udp_server:
	DOCKER_BUILDKIT=1 docker buildx build --platform=$(BUILD_PLATFORMS) --file=$(UDP_SERVER_DOCKERFILE) -t $(BLIXT_UDP_SERVER_IMAGE):$(TAG) .

.PHONY: build.image.dataplane
build.image.dataplane:
	DOCKER_BUILDKIT=1 docker buildx build --platform $(BUILD_PLATFORMS) $(BUILD_ARGS) --file=$(DATAPLANE_DOCKERFILE) -t $(BLIXT_DATAPLANE_IMAGE):$(TAG) ./

.PHONY: build.all.images
build.all.images: 
	$(MAKE) build.image.controlplane
	$(MAKE) build.image.dataplane
	$(MAKE) build.image.udp_server

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
lint: ## Lint Rust code
	cargo clippy --all -- -D warnings

# ------------------------------------------------------------------------------
# Testing
# ------------------------------------------------------------------------------

.PHONY: test
test: ## Run tests
	cargo test -vv

.PHONY: test.integration.deprecated
test.integration.deprecated: ## Run the deprecated Golang integration tests
	go clean -testcache
	BLIXT_CONTROLPLANE_IMAGE=$(BLIXT_CONTROLPLANE_IMAGE):$(TAG) \
	BLIXT_DATAPLANE_IMAGE=$(BLIXT_DATAPLANE_IMAGE):$(TAG) \
	BLIXT_UDP_SERVER_IMAGE=$(BLIXT_UDP_SERVER_IMAGE):$(TAG) \
	GOFLAGS="-tags=integration_tests" go test -race -v ./test/integration/...

.PHONY: test.icmp.integration.deprecated
test.icmp.integration.deprecated: ## Run the deprecated Golang integration tests for ICMP support
	go clean -testcache
	# This needs to run as sudo as the test involves listening for raw ICMP packets, which
	# requires you to be root.
	sudo env PATH=$(PATH) \
	BLIXT_CONTROLPLANE_IMAGE=$(BLIXT_CONTROLPLANE_IMAGE):$(TAG) \
	BLIXT_DATAPLANE_IMAGE=$(BLIXT_DATAPLANE_IMAGE):$(TAG) \
	BLIXT_UDP_SERVER_IMAGE=$(BLIXT_UDP_SERVER_IMAGE):$(TAG) \
	RUN_ICMP_TEST=true \
	go test --tags=integration_tests -run "TestUDPRouteNoReach" -race -v ./test/integration/...

# ------------------------------------------------------------------------------
# Deployment
# ------------------------------------------------------------------------------

KIND_CLUSTER ?= blixt-dev

.PHONY: install
install: manifests kustomize ## Install CRDs into the K8s cluster specified in ~/.kube/config.
	$(KUSTOMIZE) build config/crd | kubectl apply -f -

.PHONY: uninstall
uninstall: manifests kustomize ## Uninstall CRDs from the K8s cluster specified in ~/.kube/config. Call with ignore-not-found=true to ignore resource not found errors during deletion.
	$(KUSTOMIZE) build config/crd | kubectl delete --ignore-not-found=$(ignore-not-found) -f -

.PHONY: deploy
deploy: manifests kustomize ## Deploy controller to the K8s cluster specified in ~/.kube/config.
	cd config/manager && $(KUSTOMIZE) edit set image controller=${IMG}
	$(KUSTOMIZE) build config/default | kubectl apply -f -

.PHONY: undeploy
undeploy: ## Undeploy controller from the K8s cluster specified in ~/.kube/config. Call with ignore-not-found=true to ignore resource not found errors during deletion.
	$(KUSTOMIZE) build config/default | kubectl delete --ignore-not-found=$(ignore-not-found) -f -

.PHONY: build.cluster
build.cluster: $(KTF) # builds a KIND cluster which can be used for testing and development
	PATH="$(LOCALBIN):${PATH}" $(KTF) env create --name $(KIND_CLUSTER) --addon metallb

.PHONY: load.image.controlplane
load.image.controlplane: build.image.controlplane
	kubectl create namespace blixt-system || true && \
	kind load docker-image $(BLIXT_CONTROLPLANE_IMAGE):$(TAG) --name $(KIND_CLUSTER) && \
		kubectl -n blixt-system get deployment blixt-controlplane >/dev/null 2>&1 && \
		kubectl -n blixt-system rollout restart deployment blixt-controlplane || true

.PHONY: load.image.dataplane
load.image.dataplane: build.image.dataplane
	kubectl create namespace blixt-system || true && \
	kind load docker-image $(BLIXT_DATAPLANE_IMAGE):$(TAG) --name $(KIND_CLUSTER) \
		kubectl -n blixt-system rollout restart daemonset blixt-dataplane || true

.PHONY: load.all.images
load.all.images: build.all.images
	kind load docker-image $(BLIXT_CONTROLPLANE_IMAGE):$(TAG) --name $(KIND_CLUSTER) && \
	kind load docker-image $(BLIXT_DATAPLANE_IMAGE):$(TAG) --name $(KIND_CLUSTER) && \
	kind load docker-image $(BLIXT_UDP_SERVER_IMAGE):$(TAG) --name $(KIND_CLUSTER) && \
		kubectl -n blixt-system get deployment blixt-controlplane >/dev/null 2>&1 && \
		kubectl -n blixt-system rollout restart deployment blixt-controlplane || true
