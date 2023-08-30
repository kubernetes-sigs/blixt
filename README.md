![blixt-logo](https://github.com/Kong/blixt/assets/5332524/a9f54ef0-db70-4b90-a6c0-3e21d1eac37a)

> **Warning**: Experimental. There is no intention to ever make this viable for production. Do not use in production.

# Blixt

An experimental [layer 4][osi] load-balancer for [Kubernetes][k8s].

The control-plane is built using [Gateway API][gwapi] and written in
[Golang][go] with [Operator SDK][osdk]/[Controller Runtime][crn]. The
data-plane is built using [eBPF][ebpf] and is written in [Rust][rust] using
[Aya][aya].

> **Note**: The word "blixt" means "lightning" in Swedish.

[osi]:https://en.wikipedia.org/wiki/OSI_model
[k8s]:https://kubernetes.io
[gwapi]:https://gateway-api.sigs.k8s.io
[go]:https://go.dev
[osdk]:https://sdk.operatorframework.io/
[crn]:https://github.com/kubernetes-sigs/controller-runtime
[ebpf]:https://www.tigera.io/learn/guides/ebpf/ebpf-xdp/
[rust]:https://rust-lang.org
[aya]:https://aya-rs.dev

## Current Status

Current project goals are the following:

- [ ] support [Gateway][gw]/[GatewayClass][gwc] (partially complete)
- [ ] support [UDPRoute][udproute] (partially complete)
- [ ] support [TCPRoute][tcproute] (partially complete)
- [ ] use this as a basis for adding/improving [Gateway API Conformance Tests][gwcnf]
- [ ] plug this into [Gateway API][gwapi] CI to run conformance tests on PRs

After these goals are achieved, further goals may be decided.

> **Note**: [TLSRoute][tlsroute] support may be on the table, but we're looking
> for someone from the community to champion this.

> **Note**: The initial proof of concept was written as an XDP program, but
> with more features (including access to ip conntrack in newer kernels)
> available in TC, we made a switch to TC.

[gw]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1beta1.Gateway
[gwc]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1beta1.GatewayClass
[udproute]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1alpha2.UDPRoute
[tcproute]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1alpha2.TCPRoute
[gwcnf]:https://github.com/kubernetes-sigs/gateway-api/tree/main/conformance
[gwapi]:https://gateway-api.sigs.k8s.io
[tlsroute]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1alpha2.TLSRoute

## Usage

> **Note**: Currently usage is only possible on [Kubernetes In Docker
> (KIND)][kind] clusters. You can generate a new development cluster for
> testing with `make build.cluster`.

Deploy [Gateway API][gwapi] [CRDs][crds]:

```console
kubectl apply -k https://github.com/kubernetes-sigs/gateway-api/config/crd/experimental?ref=v0.7.1
```

Deploy:

```console
kubectl apply -k config/default
```

At this point you should see the `controlplane` and `dataplane` pods running
in the `blixt-system` namespace:

```console
$ kubectl -n blixt-system get pods
NAME                                 READY   STATUS    RESTARTS   AGE
blixt-controlplane-cdccc685b-9dxj2   2/2     Running   0          83s
blixt-dataplane-brsl9                1/1     Running   0          83s
```

Check the `config/samples` directory for `Gateway` and `*Route` examples you
can now deploy.

> **Note**: When developing the dataplane you can make changes in your local
> `dataplane/` directory, and within there quickly build an image and load it
> into the cluster created in the above steps with `make load.image`. This will
> build the eBPF loader and eBPF bytecode in a container image, load that image
> into the cluster, and then restart the dataplane pods to use the new build.

[kind]:https://github.com/kubernetes-sigs/kind
[gwapi]:https://github.com/kubernetes-sigs/gateway-api
[crds]:https://kubernetes.io/docs/concepts/extend-kubernetes/api-extension/custom-resources/

## Community

This project originally started at [Kong][kong] but is being [donated to
Kubernetes SIG Network][donation]. It is becoming a part of the [Gateway
API][gwapi] project and as such is discussed in the [Gateway API weekly
meetings][gwapi-meet]. In particular, we do some discussion and paired
programming of this project on the `Gateway API Code Jam` meeting which
is on the [SIG Network calendar][gwapi-meet].

You can also reach out with problems or questions by creating an
[issue][issues], or a [discussion][disc] on this repo. You can also reach out
on [Kubernetes Slack][kslack] on the `#sig-network-gateway-api` channel. There
is also a `#ebpf` channel on Kubernetes Slack for general eBPF related help.

[kong]:https://github.com/kong
[donation]:https://github.com/kong/blixt/discussions/42
[gwapi]:https://gateway-api.sigs.k8s.io/
[gwapi-meet]:https://gateway-api.sigs.k8s.io/contributing/#meetings
[issues]:https://github.com/kong/blixt/issues
[disc]:https://github.com/kong/blixt/discussions
[kslack]:https://kubernetes.slack.com

# License

The Blixt control-plane components are licensed under [Apache License, Version
2.0][apache2], which is everything _outside_ of the `dataplane/` directory. The
data-plane components are dual-licensed under the [General Public License,
Version 2.0 (only)][gplv2] and the [2-Clause BSD License][bsd2c] (at your
option) including everything _inside_ the `dataplane/` directory.

[apache2]:https://github.com/Kong/blixt/blob/main/LICENSE
[gplv2]:https://github.com/Kong/blixt/blob/main/dataplane/LICENSE.GPL-2.0
[bsd2c]:https://github.com/Kong/blixt/blob/main/dataplane/LICENSE.BSD-2-Clause
