# Build the manager binary
FROM golang:1.19 as builder

LABEL org.opencontainers.image.source=https://github.com/kong/blixt
LABEL org.opencontainers.image.description="An experimental layer 4 load-balancer built using eBPF/XDP with ebpf-go \
for use in Kubernetes via the Kubernetes Gateway API"
LABEL org.opencontainers.image.licenses=Apache-2.0

WORKDIR /workspace
# Copy the Go Modules manifests
COPY go.mod go.mod
COPY go.sum go.sum
# cache deps before building and copying source so that we don't need to re-download as much
# and so that source changes don't invalidate our downloaded layer
RUN go mod download

# Copy the go source
COPY main.go main.go
COPY controllers/ controllers/

# Build
RUN CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -a -o manager main.go

# Use distroless as minimal base image to package the manager binary
# Refer to https://github.com/GoogleContainerTools/distroless for more details
FROM gcr.io/distroless/static:nonroot
WORKDIR /
COPY --from=builder /workspace/manager .
COPY LICENSE /workspace/LICENSE
USER 65532:65532

ENTRYPOINT ["/manager"]
