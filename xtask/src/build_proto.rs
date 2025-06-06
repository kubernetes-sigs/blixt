/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

use clap::Parser;

#[derive(Debug, Parser)]
pub struct Options {}

pub(crate) fn build_proto(_opts: Options) -> Result<(), anyhow::Error> {
    let proto_file = "./dataplane/api-server/proto/backends.proto";

    println!("building proto {}", proto_file);

    tonic_build::configure()
        .protoc_arg("--experimental_allow_proto3_optional")
        .build_server(true)
        .out_dir("./dataplane/api-server/src")
        .compile_protos(&[proto_file], &["."])?;

    Ok(())
}
