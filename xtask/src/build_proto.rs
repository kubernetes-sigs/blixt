/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

use clap::Parser;
use std::process::Command;

#[derive(Debug, Parser)]
pub struct Options {}

pub(crate) fn build_proto(_opts: Options) -> Result<(), anyhow::Error> {
    let args = vec!["build", "--package", "backends"];

    let status = Command::new("cargo")
        .args(&args)
        .status()
        .expect("failed to build proto bindings");
    assert!(status.success());
    Ok(())
}
