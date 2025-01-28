/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

use std::{os::unix::process::CommandExt, process::Command};

use anyhow::Context as _;
use clap::Parser;

#[cfg(target_os = "linux")]
use crate::build_ebpf::{build_ebpf, Architecture, Options as BuildOptions};

#[derive(Debug, Parser)]
pub struct Options {
    /// Set the endianness of the BPF target
    #[cfg(target_os = "linux")]
    #[clap(default_value = "bpfel-unknown-none", long)]
    pub bpf_target: Architecture,
    /// Build and run the release target
    #[clap(long)]
    pub release: bool,
    /// The command used to wrap your application
    #[clap(short, long, default_value = "sudo -E")]
    pub runner: String,
    /// Arguments to pass to your application
    #[clap(name = "args", last = true)]
    pub run_args: Vec<String>,
}

/// Build the dataplane
#[cfg(target_os = "linux")]
fn build_dataplane(opts: &Options) -> Result<(), anyhow::Error> {
    let mut args = vec!["build", "--package", "loader"];
    if opts.release {
        args.push("--release")
    }
    let status = Command::new("cargo")
        .args(&args)
        .status()
        .expect("failed to build userspace");
    assert!(status.success());
    Ok(())
}

/// Build the controlplane
fn build_contrlplane(opts: &Options) -> Result<(), anyhow::Error> {
    let mut args = vec!["build", "--package", "controlplane"];
    if opts.release {
        args.push("--release")
    }
    let status = Command::new("cargo")
        .args(&args)
        .status()
        .expect("failed to build userspace");
    assert!(status.success());
    Ok(())
}

/// Build and run the dataplane
#[cfg(target_os = "linux")]
pub fn run_dataplane(opts: Options) -> Result<(), anyhow::Error> {
    // build our ebpf program followed by our application
    build_ebpf(BuildOptions {
        target: opts.bpf_target,
        release: opts.release,
    })
    .context("Error while building eBPF program")?;
    build_dataplane(&opts).context("Error while building dataplane's userspace application")?;

    // profile we are building (release or debug)
    let profile = if opts.release { "release" } else { "debug" };
    let bin_path = format!("target/{}/loader", profile);

    // arguments to pass to the application
    let mut run_args: Vec<_> = opts.run_args.iter().map(String::as_str).collect();

    // configure args
    let mut args: Vec<_> = opts.runner.trim().split_terminator(' ').collect();
    args.push(bin_path.as_str());
    args.append(&mut run_args);

    // spawn the command
    let err = Command::new(args.first().expect("No first argument"))
        .args(args.iter().skip(1))
        .env("RUST_LOG", "info,api_server=debug")
        .exec();

    // we shouldn't get here unless the command failed to spawn
    Err(anyhow::Error::from(err).context(format!("Failed to run `{}`", args.join(" "))))
}

pub fn run_controlplane(opts: Options) -> Result<(), anyhow::Error> {
    build_contrlplane(&opts).context("Error while building controlplane")?;

    // profile we are building (release or debug)
    let profile = if opts.release { "release" } else { "debug" };
    let bin_path = format!("target/{}/controller", profile);

    // spawn the command
    let err = Command::new(&bin_path).env("RUST_LOG", "info").exec();

    // we shouldn't get here unless the command failed to spawn
    Err(anyhow::Error::from(err).context(format!("Failed to run `{}`", bin_path)))
}
