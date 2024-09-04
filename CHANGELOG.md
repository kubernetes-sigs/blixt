# Changelog

## Table of Contents

- [v0.3.0](#v030)
- [v0.2.0](#v020)
- [v0.1.0](#v010)

## Unreleased

- The Golang control-plane has been removed, and replaced by a control-plane
  written in Rust using [kube-rs](https://github.com/kube-rs).

## v0.3.0

- A new test suite was added to run conformance tests. We now have initial
  support for running [Gateway API Conformance][gwconf] tests.
  [#92](https://github.com/Kong/blixt/pull/92)

[gwconf]:https://gateway-api.sigs.k8s.io/concepts/conformance/

## v0.2.0

This pre-release adds initial support for [TCPRoute][tcproute].

[tcproute]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1alpha2.TCPRoute

## v0.1.0

This is our first release of any kind.

This pre-release adds initial support for [UDPRoute][udproute].

[udproute]:https://gateway-api.sigs.k8s.io/references/spec/#gateway.networking.k8s.io/v1alpha2.UDPRoute
