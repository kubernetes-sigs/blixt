# Blixt DataPlane

In this directory you'll find the data-plane code for Blixt. The [extended
Berkeley Packet Filter (eBPF)][eBPF] available in the [Linux Kernel] is used as
the data-plane to support TCP and UDP ingress.

[eBPF]:https://www.kernel.org/doc/html/latest/bpf/index.html
[Linux Kernel]:https://www.kernel.org/

## Overview

In this directory you'll find the following sub-directories:

* `common` - shared libraries with common types and tools
* `ebpf` - this is where [eBPF] code lives which programs routes into the Kernel
* `loader` - a userland program which loads the eBPF code into the Kernel
* `api-server` - a [gRPC] API where configuration changes are pushed

This enables serving TCP and UDP ingress traffic.

The data-plane normally is programmed by pairing it with the control-plane,
using [TCPRoute] and [UDPRoute] APIs in Kubernetes, however as is mentioned
above it is possible to program it directly via the gRPC client if needed for
development/testing.

> **Note**: Before the gRPC API the data-plane used to pull configuration from
> the Kubernetes API instead of the configuration being pushed. The current way
> of doing things was done because it helped make development and debugging
> easier in the interim, but we expect in time to drop the gRPC API and move
> back to a Kubernetes controller design.

[eBPF]:https://www.kernel.org/doc/html/latest/bpf/index.html
[gRPC]:https://grpc.io/
[TCPRoute]:https://gateway-api.sigs.k8s.io/reference/spec/#gateway.networking.k8s.io/v1alpha2.TCPRoute
[UDPRoute]:https://gateway-api.sigs.k8s.io/reference/spec/#gateway.networking.k8s.io/v1alpha2.UDPRoute

## Development

First you'll need to create a Kubernetes cluster (with `kind`):

```console
make build.cluster
```

With that cluster from here on, you can make your changes locally, and then
build and push those changes to the cluster with:

```console
make load.image.dataplane TAG=latest
```

This will build the container image, and load it into the cluster.

Then deploy the manifest, which will create the `DaemonSet` which uses the
image you just loaded in the cluster:

```console
kubectl kustomize config/dataplane | kubectl apply -f -
```

From here on out, any time you want to push your new changes to the cluster
all you have to do is re-run:

```console
make load.image.dataplane TAG=latest
```

This will build the image, load the image, and perform a rollout to restart
the `Pods` with the new image.

To push test configurations to the data-plane you can use the [xtask] provided
in this directory which includes a `grpc-client` command for manually sending
data-plane configuration to the data-plane's [gRPC] API.

To view the documentation for this, run:

```console
cargo xtask grpc-client --help
```

> **Note**: You can alternatively deploy the control-plane to develop and test
> as well, which is helpful anyhow as any changes made here need to be
> reflected in the control-plane code eventually anyway.

[xtask]:https://docs.rs/xtasks/latest/xtasks/
[gRPC]:https://grpc.io/