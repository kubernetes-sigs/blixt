> **Warning**: Experimental. There is no intention to ever make this viable for production. Do not use in production.

# Blixt

An experimental [layer 4][osi] load-balancer for [Kubernetes][k8s] with a
control-plane built on [Gateway API][gwapi] in [Golang][go] with
[Operator SDK][osdk]/[Controller Runtime][crn], and an [eBPF][ebpf]-based
data-plane built in [Rust][rust] using [Aya][aya].

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

This is presently a work-in-progress. The project goals are currently:

- [ ] support [Gateway][gw]/[GatewayClass][gwc]
- [ ] support [UDPRoute][udproute]
- [ ] support [TCPRoute][tcproute]
- [ ] use this as a basis for adding/improving [Gateway API Conformance Tests][gwcnf]
- [ ] plug this into [Gateway API][gwapi] CI to run conformance tests on PRs

After these goals are achieved, further goals will be decided.

> **Note**: [TLSRoute][tlsroute] support may be on the table, but we're looking
> for someone from the community to champion this.

> **Note**: The initial proof of concept was written as an XDP program, but
> with more features (including access to ip conntrack in newer kernels)
> available in TC, the maintainers are most likely going to be converting
> this to a TC program soon.

> **Note**: There is an open question as to whether the data-plane should be
> implemented standalone behind the `Gateway` resources, or if it might make
> any sense or be advantageous to implement it as a backend for [KPNG][kpng].
> This is something the maintainers intend to determine before a `v1` release.

[gw]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1beta1.Gateway
[gwc]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1beta1.GatewayClass
[udproute]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1alpha2.UDPRoute
[tcproute]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1alpha2.TCPRoute
[gwcnf]:https://github.com/kubernetes-sigs/gateway-api/tree/main/conformance
[gwapi]:https://gateway-api.sigs.k8s.io
[tlsroute]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1alpha2.TLSRoute
[kpng]:https://github.com/kubernetes-sigs/kpng

## Usage

Deploy [Gateway API][gwapi] [CRDs][crds]:

```console
kubectl kustomize https://github.com/kubernetes-sigs/gateway-api/config/crd/experimental | kubectl apply -f -
```

Deploy:

```console
kubectl kustomize config/default | kubectl apply -f -
```

[gwapi]:https://github.com/kubernetes-sigs/gateway-api
[crds]:https://kubernetes.io/docs/concepts/extend-kubernetes/api-extension/custom-resources/

## Community

This project originally started at [Kong][kong] but is being [donated to
Kubernetes SIG Network][donation]. It is becoming a part of the [Gateway
API][gwapi] project and as such is discussed in the [Gateway API weekly
meetings][gwapi-meet].In particular, we do some discussion and paired
programming of this project on the `Gateway API Code Jam` meeting which
is on the [calendar][gwapi-meet] for every Friday.

You can also reach out with problems or questions by creating an
[issue][issues], or a [discussion][disc] on this repo. You can also reach out
on [Kubernetes Slack][kslack] on the `#sig-network-gateway-api` channel.

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
data-plane components are licensed under the [General Public License, Version
2.0 (only)][gplv2], which includes everything _inside_ the `dataplane/`
directory.

[apache2]:https://github.com/Kong/blixt/blob/main/LICENSE
[gplv2]:https://github.com/Kong/blixt/blob/main/dataplane/LICENSE
