# ARCHIVE

**This project is now concluded**, and archived. Blixt started in the early
2020's at a time when eBPF technology was a huge buzz for Kubernetes, and
members of the Kubernetes SIG Network community wanted to experiment with and
explore the technology on K8s. Over time we suggested some specific goals we
_could_ have for the project, but those never really stuck. The project operated
primarily as an experimental sandbox, and a "just for fun" project.

As such if you're reading this, we're glad if it helps you and provides some
interesting insights, but do note that much of what you'll find in this
repository is largely incomplete exploratory code which can only be used in
a limited environment, so just keep that in mind.

We had a lot of fun working on this while it was active. It was great to create
the first official Kubernetes project in Rust, and experimenting with eBPF in
its nascence was exciting. All things must come to an end however. Thank you to
everyone who contributed to the project, good times!

# Blixt

A [layer 4][osi] load-balancer for [Kubernetes] written in [Rust] using
[kube-rs] for the control-plane and [eBPF] with [aya] for the data-plane.

[osi]:https://en.wikipedia.org/wiki/OSI_model
[Kubernetes]:https://kubernetes.io
[Rust]:https://rust-lang.org
[Kube-RS]:https://github.com/kube-rs
[eBPF]:https://ebpf.io/what-is-ebpf/
[Aya]:https://aya-rs.dev

# License

The Blixt control-plane components are licensed under [Apache License, Version
2.0][apache2], which is everything _outside_ of the `dataplane/` directory. The
data-plane components are dual-licensed under the [General Public License,
Version 2.0 (only)][gplv2] and the [2-Clause BSD License][bsd2c] (at your
option) including everything _inside_ the `dataplane/` directory.

[apache2]:https://github.com/kubernetes-sigs/blixt/blob/main/LICENSE
[gplv2]:https://github.com/kubernetes-sigs/blixt/blob/main/dataplane/LICENSE.GPL-2.0
[bsd2c]:https://github.com/kubernetes-sigs/blixt/blob/main/dataplane/LICENSE.BSD-2-Clause
