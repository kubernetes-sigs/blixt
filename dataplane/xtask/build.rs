/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

fn main() {
    let proto_file = "../api-server/proto/backends.proto";

    tonic_build::configure()
        .protoc_arg("--experimental_allow_proto3_optional")
        .protoc_arg("--proto_path=..")
        .compile(&[proto_file], &["."])
        .unwrap_or_else(|e| panic!("protobuf compile error: {}", e));
}
