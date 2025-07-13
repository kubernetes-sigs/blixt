/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

use std::net::Ipv4Addr;

use anyhow::Context;
use api_server::config::TLSConfig;
use api_server::start as start_api_server;
use aya::maps::HashMap;
use aya::programs::{SchedClassifier, TcAttachType, tc};
use aya::{Ebpf, include_bytes_aligned};
use aya_log::EbpfLogger;
use clap::Parser;
use common::{BackendKey, BackendList, ClientKey, LoadBalancerMapping};
use log::{info, warn};

/// Command-line options for the application.
///
/// This struct defines the options available for the command-line interface,
/// including an interface name (`iface`) and an optional TLS configuration (`tls_config`).
#[derive(Debug, Parser)]
struct Opt {
    /// Name of the network interface to attach the eBPF programs to.
    ///
    /// By default, this is set to `"lo"` (the loopback interface).
    #[clap(short, long, default_value = "lo")]
    iface: String,
    /// Optional TLS configuration for securing the API server.
    ///
    /// If no TLS configuration is provided, the server will start without TLS.
    /// You can specify either `tls` for server-only TLS or `mutual-tls` for mutual TLS.
    #[clap(subcommand)]
    tls_config: Option<TLSConfig>,
}

/// Main function for the application.
///
/// This function sets up and runs eBPF programs on the specified network interface
/// and optionally configures TLS for the API server.
///
/// The program supports an optional TLS configuration, allowing the user to choose between:
/// - `tls`: Server-only TLS.
/// - `mutual-tls`: Mutual TLS, where both server and client authenticate with certificates.
///
/// # Arguments
///
/// - `iface`: The network interface to attach the eBPF programs to.
/// - `tls_config`: Optional subcommand to configure TLS for the API server.
///
/// # Example
///
/// ```bash
/// # Running with default interface and no TLS config:
/// $ dataplane
///
/// # Running with a specified interface and server-only TLS config:
/// $ dataplane --iface eth0 tls --server-certificate-path /path/to/cert --server-private-key-path /path/to/key
///
/// # Running with mutual TLS config:
/// $ dataplane --iface eth0 mutual-tls --server-certificate-path /path/to/cert --server-private-key-path /path/to/key --client-certificate-authority-root-path /path/to/ca
/// ```
#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let opt = Opt::parse();

    env_logger::init();

    info!("loading ebpf programs");

    #[cfg(debug_assertions)]
    let mut bpf_program = Ebpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/debug/loader"
    ))?;
    #[cfg(not(debug_assertions))]
    let mut bpf_program = Ebpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/release/loader"
    ))?;
    if let Err(e) = EbpfLogger::init(&mut bpf_program) {
        warn!("failed to initialize eBPF logger: {e}");
    }

    info!("attaching tc_ingress program to {}", &opt.iface);

    let _ = tc::qdisc_add_clsact(&opt.iface);
    let ingress_program: &mut SchedClassifier =
        bpf_program.program_mut("tc_ingress").unwrap().try_into()?;
    ingress_program.load()?;
    ingress_program
        .attach(&opt.iface, TcAttachType::Ingress)
        .context("failed to attach the ingress TC program")?;

    info!("attaching tc_egress program to {}", &opt.iface);

    let egress_program: &mut SchedClassifier =
        bpf_program.program_mut("tc_egress").unwrap().try_into()?;
    egress_program.load()?;
    egress_program
        .attach(&opt.iface, TcAttachType::Egress)
        .context("failed to attach the egress TC program")?;

    info!("starting api server");
    info!("Using tls config: {:?}", &opt.tls_config);
    let backends: HashMap<_, BackendKey, BackendList> = HashMap::try_from(
        bpf_program
            .take_map("BACKENDS")
            .expect("no maps named BACKENDS"),
    )?;
    let gateway_indexes: HashMap<_, BackendKey, u16> = HashMap::try_from(
        bpf_program
            .take_map("GATEWAY_INDEXES")
            .expect("no maps named GATEWAY_INDEXES"),
    )?;
    let tcp_conns: HashMap<_, ClientKey, LoadBalancerMapping> = HashMap::try_from(
        bpf_program
            .take_map("LB_CONNECTIONS")
            .expect("no maps named LB_CONNECTIONS"),
    )?;

    start_api_server(
        Ipv4Addr::new(0, 0, 0, 0),
        9874,
        backends,
        gateway_indexes,
        tcp_conns,
        opt.tls_config,
    )
    .await?;

    info!("Exiting...");

    Ok(())
}
