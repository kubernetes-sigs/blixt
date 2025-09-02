/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

use std::net::Ipv4Addr;
use std::path::Path;

use api_server::config::TLSConfig;
use api_server::start as start_api_server;
use aya::maps::{HashMap, Map, MapData};
use aya::programs::{ProgramError, SchedClassifier, TcAttachType, tc};
use aya::{Ebpf, include_bytes_aligned};
use aya_log::EbpfLogger;
use clap::Parser;
use common::{BackendKey, BackendList, ClientKey, LoadBalancerMapping};
use thiserror::Error as ThisError;
use tracing::{debug, info, trace};
use tracing_log::LogTracer;
use tracing_subscriber::EnvFilter;

/// Command-line options for the application.
///
/// This struct defines the options available for the command-line interface,
/// including an interface name (`iface`) and an optional TLS configuration (`tls_config`).
#[derive(Debug, Parser)]
struct Opt {
    /// Name of the network interface to attach the eBPF programs to.
    ///
    /// By default, this is set to `lo` (the loopback interface).
    #[clap(short, long, default_value = "lo")]
    iface: String,
    /// Optional TLS configuration for securing the API server.
    ///
    /// If no TLS configuration is provided, the server will start without TLS.
    /// You can specify either `tls` for server-only TLS or `mutual-tls` for mutual TLS.
    #[clap(subcommand)]
    tls_config: Option<TLSConfig>,

    /// Load eBPF programs and maps
    ///
    /// Overrides usage of pinned programs/maps during init.
    ///
    /// WARN: loading resets all the dataplane configuration and interrupts traffic flow
    #[clap(long)]
    load_ebpf: bool,
}

#[derive(ThisError, Debug)]
enum LoaderError {
    #[error("Failed to load ebpf map {0}")]
    MapLoad(String),
    #[error("Failed to pin ebpf {0} {1} {2}")]
    Pin(String, String, String),
    #[error("Could not find {0} {1}")]
    NotFound(String, String),
    #[error("{0}")]
    Program(#[from] ProgramError),
    #[error("{0}")]
    AyaLog(#[from] aya_log::Error),
}

type Result<T, E = LoaderError> = std::result::Result<T, E>;

const EBPF_FS_ROOT: &str = "/sys/fs/bpf";

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
/// - `load_ebpf`: load the eBPF programs and maps even in case pinned objects are available
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
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_file(true)
        .with_line_number(true)
        .init();

    LogTracer::init()?;

    let opts = Opt::parse();
    info!("{:?}", opts);

    info!("Loading ebpf programs");
    #[cfg(debug_assertions)]
    let mut bpf_program = Ebpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/debug/loader"
    ))?;
    #[cfg(not(debug_assertions))]
    let mut bpf_program = Ebpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/release/loader"
    ))?;

    let _ = tc::qdisc_add_clsact(&opts.iface);
    let mut ingress_program = get_pinned_program("tc_ingress")?;
    program_load_pin(
        &mut bpf_program,
        &mut ingress_program,
        "tc_ingress",
        TcAttachType::Ingress,
        &opts.iface,
        opts.load_ebpf,
    )?;

    let mut egress_program = get_pinned_program("tc_egress")?;
    program_load_pin(
        &mut bpf_program,
        &mut egress_program,
        "tc_egress",
        TcAttachType::Egress,
        &opts.iface,
        opts.load_ebpf,
    )?;

    let backends_map = map_take_pin(&mut bpf_program, "BACKENDS", opts.load_ebpf)?;
    let gateway_indexes_map = map_take_pin(&mut bpf_program, "GATEWAY_INDEXES", opts.load_ebpf)?;
    let tcp_conns_map = map_take_pin(&mut bpf_program, "LB_CONNECTIONS", opts.load_ebpf)?;

    let backends: HashMap<MapData, BackendKey, BackendList> = HashMap::try_from(backends_map)?;
    trace!("Existing backends:");
    for k in backends.keys() {
        let k = k?;
        trace!("{:?}", k);
    }

    let gateway_indexes: HashMap<MapData, BackendKey, u16> =
        HashMap::try_from(gateway_indexes_map)?;
    trace!("Existing gateway_indexes:");
    for k in gateway_indexes.keys() {
        let k = k?;
        trace!("{:?}", k);
    }

    let tcp_conns: HashMap<MapData, ClientKey, LoadBalancerMapping> =
        HashMap::try_from(tcp_conns_map)?;
    trace!("Existing tcp_conns:");
    for k in tcp_conns.keys() {
        let k = k?;
        trace!("{:?}", k);
    }

    info!("Starting api server");
    info!("Using tls config: {:?}", &opts.tls_config);
    start_api_server(
        Ipv4Addr::new(0, 0, 0, 0),
        9874,
        backends,
        gateway_indexes,
        tcp_conns,
        opts.tls_config,
    )
    .await?;

    info!("Exiting...");
    Ok(())
}

fn program_load_pin(
    bpf_program: &mut Ebpf,
    pinned_program: &mut Option<SchedClassifier>,
    identifier: &str,
    tc_attach_type: TcAttachType,
    iface: &str,
    load_ebpf: bool,
) -> Result<()> {
    if pinned_program.is_some() && !load_ebpf {
        let program = pinned_program.as_mut().ok_or(LoaderError::NotFound(
            "program".to_string(),
            identifier.to_string(),
        ))?;
        attach_interface_logs(identifier, iface, tc_attach_type, program)?;
    } else {
        let program = load_pin_program(bpf_program, identifier, load_ebpf)?;
        attach_interface_logs(identifier, iface, tc_attach_type, program)?;
    };
    Ok(())
}

fn get_pinned_program(identifier: &str) -> Result<Option<SchedClassifier>> {
    let path = format!("{EBPF_FS_ROOT}/{identifier}");
    let pin_path = Path::new(&path);

    if pin_path
        .try_exists()
        .map_err(|e| LoaderError::Pin("program".to_string(), path.clone(), e.to_string()))?
    {
        debug!("ebpf program {identifier} is already pinned to {path}");
        let program = SchedClassifier::from_pin(pin_path).map_err(LoaderError::Program)?;
        info!("Loaded ebpf program {identifier} from pin {path}");
        return Ok(Some(program));
    }

    Ok(None)
}

fn load_pin_program<'a>(
    bpf_program: &'a mut Ebpf,
    identifier: &str,
    load_ebpf: bool,
) -> Result<&'a mut SchedClassifier> {
    let program: &mut SchedClassifier = bpf_program
        .program_mut(identifier)
        .ok_or(LoaderError::NotFound(
            "program".to_string(),
            identifier.to_string(),
        ))?
        .try_into()?;
    info!("Loaded ebpf program {identifier}");

    let path = format!("{EBPF_FS_ROOT}/{identifier}");
    let pin_path = Path::new(&path);

    // loading ebpf requested
    // removing pinned program in case existing
    if load_ebpf
        && pin_path.try_exists().map_err(|e| {
            LoaderError::Pin("program".to_string(), identifier.to_string(), e.to_string())
        })?
    {
        info!("Removing existing pinned program {}", path);
        std::fs::remove_file(pin_path).map_err(|e| {
            LoaderError::Pin("program".to_string(), identifier.to_string(), e.to_string())
        })?;
    }

    program.load()?;

    program
        .pin(pin_path)
        .map_err(|e| LoaderError::Pin("program".to_string(), path.clone(), e.to_string()))?;
    info!("Successfully pinned ebpf program {identifier} to {path}");

    Ok(program)
}

fn attach_interface_logs(
    identifier: &str,
    iface: &str,
    tc_attach_type: TcAttachType,
    program: &mut SchedClassifier,
) -> Result<()> {
    info!("Attaching {identifier} program to {}", iface);
    program
        .attach(iface, tc_attach_type)
        .map_err(LoaderError::Program)?;
    info!("Initializing logs for {identifier} program");
    let info = program.info()?;
    EbpfLogger::init_from_id(info.id())?;
    Ok(())
}

fn map_take_pin(bpf_program: &mut Ebpf, identifier: &str, load_ebpf: bool) -> Result<Map> {
    let path = format!("{EBPF_FS_ROOT}/{identifier}");
    let pin_path = Path::new(&path);
    let pin_path_exists = pin_path
        .try_exists()
        .map_err(|e| LoaderError::Pin("map".to_string(), identifier.to_string(), e.to_string()))?;

    if !load_ebpf && pin_path_exists {
        debug!("ebpf map {identifier} is already pinned to {path}");
        let map_data = MapData::from_pin(pin_path).map_err(|e| {
            LoaderError::MapLoad(format!("failed to load map from pin {path}: {e}"))
        })?;
        info!("Loaded ebpf map {identifier} from pin {path}");
        Ok(Map::HashMap(map_data))
    } else {
        if pin_path_exists {
            info!("Removing existing pinned map {}", path);
            std::fs::remove_file(pin_path).map_err(|e| {
                LoaderError::Pin("map".to_string(), identifier.to_string(), e.to_string())
            })?;
        }
        info!("Loaded ebpf map {identifier}");
        let map = bpf_program
            .take_map(identifier)
            .ok_or(LoaderError::MapLoad(identifier.to_string()))?;
        info!("Successfully pinned ebpf map {identifier} to {path}");
        map.pin(pin_path).map_err(|e| {
            LoaderError::Pin("map".to_string(), identifier.to_string(), e.to_string())
        })?;
        Ok(map)
    }
}
