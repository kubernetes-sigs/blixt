![blixt](https://github.com/kubernetes-sigs/blixt/assets/5332524/387ce94a-88fd-43a9-bde9-73fb9005564d)

> **Warning**: The `main` branch is under heavy development due to a [rewrite].
> **Warning**: Experimental. **DO NOT USE IN PRODUCTION**.

[rewrite]:https://github.com/kubernetes-sigs/blixt/milestone/8

# Blixt

A [layer 4][osi] load-balancer for [Kubernetes] written in [Rust] using
[kube-rs] for the control-plane and [eBPF] with [aya] for the data-plane.

> **Note**: The word "blixt" means "lightning" in Swedish.

[osi]:https://en.wikipedia.org/wiki/OSI_model
[Kubernetes]:https://kubernetes.io
[Rust]:https://rust-lang.org
[Kube-RS]:https://github.com/kube-rs
[eBPF]:https://ebpf.io/what-is-ebpf/
[Aya]:https://aya-rs.dev

## Current Status

We're currently focused on getting some of the core functionality in place. The
immediate goals are to add:

- [ ] support for the [Kubernetes Service API] ([upcoming])
- [ ] support for [Gateway], [GatewayClass], [UDPRoute], [TCPRoute] (in progress, partially complete)

After these goals are achieved, further goals may be decided.

[Kubernetes Service API]:https://kubernetes.io/docs/concepts/services-networking/service/
[upcoming]:https://github.com/kubernetes-sigs/blixt/issues/279
[Gateway]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1beta1.Gateway
[GatewayClass]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1beta1.GatewayClass
[UDPRoute]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1alpha2.UDPRoute
[TCPRoute]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1alpha2.TCPRoute

## Usage

> **Note**: We don't host container images for this project. You **must** build,
> load (or host) the images yourself.

> **Warning**: We currently only support [Kubernetes In Docker (KIND)] clusters.

Deploy the [Gateway API] [CRDs]:

```console
kubectl apply -k https://github.com/kubernetes-sigs/gateway-api/config/crd/experimental?ref=v1.2.1
```

Build container images:

```console
make build.all.images TAG=latest
```

Load images into your Kind cluster:

```console
make load.all.images TAG=latest
```

Deploy Blixt:

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

> **Note**: Check the `config/samples` directory for `Gateway` and `*Route`
> examples you can now deploy.

[Kubernetes In Docker (KIND)]:https://github.com/kubernetes-sigs/kind
[Gateway API]:https://github.com/kubernetes-sigs/gateway-api
[CRDs]:https://kubernetes.io/docs/concepts/extend-kubernetes/api-extension/custom-resources/

## Development

Development is generally done by making your changes locally, building images
with those changes locally and then deploying those images to a local `kind`
cluster (see the usage section above to get an environment set up).

You can build the data-plane:

```console
make build.image.dataplane
```

Then load it into the cluster and perform a rollout on the `Daemonset`:

```console
make load.image.dataplane
```

The same can be done for the control-plane:

```console
make build.image.controlplane
make load.image.controlplane
```

## Community

Reach by creating [issues] or [discussions]. We also have the `#blixt` channel
on [Kubernetes Slack].

[issues]:https://github.com/kubernetes-sigs/blixt/issues
[discussions]:https://github.com/kubernetes-sigs/blixt/discussions
[Kubernetes Slack]:https://kubernetes.slack.com

# License

The Blixt control-plane components are licensed under [Apache License, Version
2.0][apache2], which is everything _outside_ of the `dataplane/` directory. The
data-plane components are dual-licensed under the [General Public License,
Version 2.0 (only)][gplv2] and the [2-Clause BSD License][bsd2c] (at your
option) including everything _inside_ the `dataplane/` directory.

[apache2]:https://github.com/kubernetes-sigs/blixt/blob/main/LICENSE
[gplv2]:https://github.com/kubernetes-sigs/blixt/blob/main/dataplane/LICENSE.GPL-2.0
[bsd2c]:https://github.com/kubernetes-sigs/blixt/blob/main/dataplane/LICENSE.BSD-2-Clause
