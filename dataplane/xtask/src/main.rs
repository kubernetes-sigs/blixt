/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

// Remember to run `cargo install bindgen-cli`

mod build_ebpf;
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
    BuildEbpf(build_ebpf::Options),
    Run(run::Options),
    GrpcClient(grpc::Options),
}

#[tokio::main]
async fn main() {
    let opts = Options::parse();

    use Command::*;
    let ret = match opts.command {
        BuildEbpf(opts) => build_ebpf::build_ebpf(opts),
        Run(opts) => run::run(opts),
        GrpcClient(opts) => grpc::update(opts).await,
    };

    if let Err(e) = ret {
        eprintln!("{:#}", e);
        exit(1);
    }
}
