# Rust Controlplane

This directory hosts the code for the WIP Rust controlplane. It aims to port the Golang reconcilers in `blixt/controllers` in Rust.
For more context behind the decision, please see https://github.com/kubernetes-sigs/blixt/issues/176 and https://github.com/kubernetes-sigs/blixt/discussions/150.

## Progress

- [ ] GatewayClass reconciler
- [x] Gateway reconciler
- [ ] TCPRoute reconciler
- [ ] UDPRoute reconciler

## Getting started

* Create a Kubernetes cluster

```bash
make build.cluster
```

* Install Gateway API CRDS; create a `GatewayClass` and `Gateway`

```bash
kubectl apply -k https://github.com/kubernetes-sigs/gateway-api/config/crd/experimental\?ref\=v1.0.0
kubectl apply -f config/samples/gateway_v1.yaml
```

* Run the reconciler

```bash
cd controlplane
make run
```
