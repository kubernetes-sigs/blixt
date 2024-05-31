![blixt](https://github.com/kubernetes-sigs/blixt/assets/5332524/387ce94a-88fd-43a9-bde9-73fb9005564d)

> **Warning**: Experimental. There is no intention to ever make this viable for production. Do not use in production.

# Blixt

An experimental [layer 4][osi] load-balancer for [Kubernetes][k8s].

The control-plane is built using [Gateway API][gwapi] and written in
[Golang][go] with [Operator SDK][osdk]/[Controller Runtime][crn]. The
data-plane is built using [eBPF][ebpf] and is written in [Rust][rust] using
[Aya][aya].

> **Warning**: We've [decided](https://github.com/kubernetes-sigs/blixt/discussions/150) that we're going to rewrite
> the control-plane in Rust (as it was earlier on in this project's life), so please note that if you contribute to
> the Go control-plane in the interim before we take this warning down, things might get "lost" when we switch to the
> new version. See the [relevant milestone](https://github.com/kubernetes-sigs/blixt/milestone/8) and check in with
> us in the issues (or via discussions) if you're interested in working on something control-plane related!

> **Note**: We use [Cargo workspaces](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html) 
> to manage the various crates spread across the Rust parts of the repo. However, there is one exception.
> The `dataplane/eBPF` crate must be kept as a standalone because it needs to re-implement the `panic` handler.
> All new crates should be added to the workspace, if possible.
 
This project's main purposes are to help facilitate the development of the
[Gateway API][gwapi] project and to be a fun and safe place for contributors to
contribute and try out newer technologies.

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

Given the goals and nature of this project, and the fact that everyone who works
on it is a volunteer, we try to optimize for time with a highly iterative
development approach. This project follows a **"Work -> Right -> Fast"** development
mentality, which is to say for any functionality or feature we focus on making sure
it **_works_** at a basic level first, then we'll focus on making it **_work right_**,
and then once we're happy with the code quality we'll move on to making it **_faster_**
and more efficient. This project is **currently still very much in the early parts of
the work stage and so the code may be a little rough and/or incomplete**. We would love
to have _you_ join us in iterating on it and helping us build it together!

> **Note**: [TLSRoute][tlsroute] support may be on the table, but we're looking
> for someone from the community to champion this.

> **Note**: [HTTPRoute][httproute] support may be on the table, but we're looking
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
[httproute]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1beta1.HTTPRoute

## Usage

> **Note**: Currently usage is only possible on [Kubernetes In Docker
> (KIND)][kind] clusters. You can generate a new development cluster for
> testing with `make build.cluster`.

> **Note**: Currently our container images are under migration from a private repository.
> At this moment, you should build and load images yourself.

1. Deploy [Gateway API][gwapi] [CRDs][crds]:

```console
kubectl apply -k https://github.com/kubernetes-sigs/gateway-api/config/crd/experimental?ref=v0.8.1
```

2. Build Blixt images:

```console
make build.all.images TAG=latest
```

3. Load images into your Kind cluster:

```console
make load.all.images TAG=latest
```

4. Deploy Blixt:

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

You can reach out to the community by creating [issue][issues] or
[discussions][disc]. You can also reach out on [Kubernetes Slack][kslack] on the
`#blixt` channel. There is also a `#ebpf` channel on Kubernetes Slack for general
eBPF related help.

[donation]:https://github.com/kubernetes/org/issues/3875
[gwapi]:https://gateway-api.sigs.k8s.io/
[gwapi-meet]:https://gateway-api.sigs.k8s.io/contributing/#meetings
[issues]:https://github.com/kubernetes-sigs/blixt/issues
[disc]:https://github.com/kubernetes-sigs/blixt/discussions
[kslack]:https://kubernetes.slack.com

# License

The Blixt control-plane components are licensed under [Apache License, Version
2.0][apache2], which is everything _outside_ of the `dataplane/` directory. The
data-plane components are dual-licensed under the [General Public License,
Version 2.0 (only)][gplv2] and the [2-Clause BSD License][bsd2c] (at your
option) including everything _inside_ the `dataplane/` directory.

[apache2]:https://github.com/kubernetes-sigs/blixt/blob/main/LICENSE
[gplv2]:https://github.com/kubernetes-sigs/blixt/blob/main/dataplane/LICENSE.GPL-2.0
[bsd2c]:https://github.com/kubernetes-sigs/blixt/blob/main/dataplane/LICENSE.BSD-2-Clause
