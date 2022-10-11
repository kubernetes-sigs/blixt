> **Warning**: Work in progress (WIP)

> **Warning**: Experimental. Do not use in production.

# Blixt

An experimental [layer 4][osi] load-balancer built using [eBPF][ebpf] with
[ebpf-go][ebpf-go] for use in [Kubernetes][k8s] via the [Kubernetes Gateway
API][gwapi].

> **Note**: The word "blixt" means "lightning" in Swedish.

[osi]:https://en.wikipedia.org/wiki/OSI_model
[ebpf]:https://www.tigera.io/learn/guides/ebpf/ebpf-xdp/
[ebpf-go]:https://github.com/cilium/ebpf
[k8s]:https://kubernetes.io
[gwapi]:https://github.com/kubernetes-sigs/gateway-api

## Current Status

This is presently a work-in-progress. The intention for now is to create a
proof-of-concept which achieves the following:

- [ ] can support the specification of [Gateway][gw]/[GatewayClass][gwc]
- [ ] can support the full specification of [UDPRoute][udproute]
- [ ] can support the full specification of [TCPRoute][tcproute]
- [ ] (MAYBE?) support the full specification of [TLSRoute][tlsroute]

After these goals are achieved, further goals will be decided. Until then this
should be considered only a fun experiment, and used for nothing more.

[gw]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1beta1.Gateway
[gwc]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1beta1.GatewayClass
[udproute]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1alpha2.UDPRoute
[tcproute]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1alpha2.TCPRoute
[tlsroute]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1alpha2.TLSRoute

## Usage

Deploy [Gateway API][gwapi] [CRDs][crds]:

```console
$ kubectl kustomize https://github.com/kubernetes-sigs/gateway-api/config/crd | kubectl apply -f -
```

Deploy:

```console
$ kubectl kustomize config/default | kubectl apply -f -
```

[gwapi]:https://github.com/kubernetes-sigs/gateway-api
[crds]:https://kubernetes.io/docs/concepts/extend-kubernetes/api-extension/custom-resources/

# License

The Blixt control-plane components are licensed under [Apache License, Version
2.0][apache2], which is everything _outside_ of the `dataplane/` directory. The
data-plane components are licensed under the [General Public License, Version
2.0 (only)][gplv2], which includes everything _inside_ the `dataplane/`
directory.

[apache2]:https://github.com/Kong/blixt/blob/main/LICENSE
[gplv2]:https://github.com/Kong/blixt/blob/main/dataplane/LICENSE
