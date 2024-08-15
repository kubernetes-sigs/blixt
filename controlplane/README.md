# Rust Controlplane

This directory hosts the code for the WIP Rust controlplane. It aims to port the
Golang reconcilers in `blixt/controllers` in Rust. For more context behind the
decision, please see https://github.com/kubernetes-sigs/blixt/issues/176 and
https://github.com/kubernetes-sigs/blixt/discussions/150.

## Progress

- [ ] GatewayClass reconciler
- [x] Gateway reconciler
- [ ] TCPRoute reconciler
- [ ] UDPRoute reconciler

## Getting started

First you'll need to create a Kubernetes cluster (with `kind`):

```console
make build.cluster
```

You'll need a copy of the dataplane running on the cluster for the controlplane
to communicate with. To make this happen first we'll need to build a dataplane
image, and load it into the cluster:

```console
make load.image.dataplane
```

Then deploy the dataplane `DaemonSet`:

```console
kubectl kustomize config/dataplane | kubectl apply -f -
```

Now that the dataplane is running, we can fire up the controlplane. First
install the Gateway API CRDs on the new cluster:

```console
kubectl apply -k https://github.com/kubernetes-sigs/gateway-api/config/crd/experimental\?ref\=v1.0.0
```

Then create a `GatewayClass` and `Gateway` using our sample manifests:

```console
kubectl apply -f config/samples/gateway_v1.yaml
```

Now run the reconciler locally:

```console
cd controlplane
make run
```

You should see the `Gateway` resource move to status `programmed` and receive
an IP address:

```console
$ k get gateways
NAME                    CLASS                   ADDRESS        PROGRAMMED   AGE
blixt-tcproute-sample   blixt-tcproute-sample   172.18.128.1   True         5m23s
```

Now you can attach `TCPRoutes` and `UDPRoutes` to it:

> **TODO**: `TCPRoute` & `UDPRoute`
