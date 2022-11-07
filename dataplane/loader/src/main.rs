use anyhow::Context;
use aya::programs::{tc, SchedClassifier, TcAttachType};
use aya::{include_bytes_aligned, Bpf};
use aya_log::BpfLogger;
use clap::Parser;
use log::{info, warn};
use tokio::signal;

#[derive(Debug, Parser)]
struct Opt {
    #[clap(short, long, default_value = "lo")]
    iface: String,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let opt = Opt::parse();

    env_logger::init();

    #[cfg(debug_assertions)]
    let mut bpf = Bpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/debug/loader"
    ))?;
    #[cfg(not(debug_assertions))]
    let mut bpf = Bpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/release/loader"
    ))?;
    if let Err(e) = BpfLogger::init(&mut bpf) {
        warn!("failed to initialize eBPF logger: {}", e);
    }
    let _ = tc::qdisc_add_clsact(&opt.iface);
    let program: &mut SchedClassifier = bpf.program_mut("tc_ingress").unwrap().try_into()?;
    program.load()?;
    program
        .attach(&opt.iface, TcAttachType::Ingress)
        .context("failed to attach the TCP program")?;

    info!("Waiting for Ctrl-C...");
    signal::ctrl_c().await?;
    info!("Exiting...");

    Ok(())
}
