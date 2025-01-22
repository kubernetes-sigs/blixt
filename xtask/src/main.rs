/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

// Remember to run `cargo install bindgen-cli`

#[cfg(target_os = "linux")]
mod build_ebpf;
mod build_proto;
mod grpc;
mod run;

use std::process::exit;

use clap::Parser;

#[derive(Debug, Parser)]
pub struct Options {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Parser)]
enum Command {
    #[cfg(target_os = "linux")]
    BuildEbpf(build_ebpf::Options),
    #[cfg(target_os = "linux")]
    RunDataplane(run::Options),

    RunControlplane(run::Options),
    BuildProto(build_proto::Options),
    GrpcClient(grpc::Options),
}

#[tokio::main]
async fn main() {
    let opts = Options::parse();

    use Command::*;
    #[cfg(target_os = "linux")]
    let ret = match opts.command {
        BuildEbpf(opts) => build_ebpf::build_ebpf(opts),
        BuildProto(opts) => build_proto::build_proto(opts),
        RunDataplane(opts) => run::run_dataplane(opts),
        RunControlplane(opts) => run::run_controlplane(opts),
        GrpcClient(opts) => grpc::update(opts).await,
    };

    #[cfg(not(target_os = "linux"))]
    let ret = match opts.command {
        BuildProto(opts) => build_proto::build_proto(opts),
        RunControlplane(opts) => run::run_controlplane(opts),
        GrpcClient(opts) => grpc::update(opts).await,
    };

    if let Err(e) = ret {
        eprintln!("{:#}", e);
        exit(1);
    }
}
