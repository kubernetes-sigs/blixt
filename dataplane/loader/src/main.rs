/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

use std::fs;
use std::net::Ipv4Addr;

use anyhow::Context;
use api_server::start as start_api_server;
use aya::maps::HashMap;
use aya::programs::{tc, SchedClassifier, TcAttachType};
use aya::{include_bytes_aligned, Ebpf};
use aya_log::EbpfLogger;
use clap::Parser;
use common::{BackendKey, BackendList, ClientKey, LoadBalancerMapping};
use log::{info, warn};

#[derive(Debug, Parser)]
struct Opt {
    #[clap(short, long, default_value = "lo")]
    iface: String,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let opt = Opt::parse();

    env_logger::init();

    info!("loading ebpf programs");

    let path = "../../target/bpfel-unknown-none/debug/loader";
    let bytes = fs::read(path);

    #[cfg(debug_assertions)]
    let mut bpf_program = Ebpf::load(&bytes)?;
    
    #[cfg(not(debug_assertions))]
    let mut bpf = Ebpf::load(&bytes)?;
    if let Err(e) = EbpfLogger::init(&mut bpf_program) {
        warn!("failed to initialize eBPF logger: {}", e);
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
    )
    .await?;

    info!("Exiting...");

    Ok(())
}
