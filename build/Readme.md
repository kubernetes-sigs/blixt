# Development Image Build
## Goals
- Fast build.
- Fast deployment.
- Not to be used for publishing or releases.
- Avoid long compile times or code recompiles when deploying onto local kind cluster.

## xtask
The `container-image` task supports `build`, `load` and `start` actions.

## Setup
- Uses binaries compiled by the developers cargo setup.
- Uses standard libc setup.
- Mounts the cargo workspace as volume to the containers `/workspace`.
- Images tags default to `latest`.
- Based on `debian:trixie-slim` to avoid `glibc` and `libgcc` symbol issues with Rust nightly.