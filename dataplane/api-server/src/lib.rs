/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

pub mod backends;
pub mod config;
pub mod netutils;
pub mod server;

use std::{
    fs,
    net::{Ipv4Addr, SocketAddrV4},
};

use anyhow::{Context, Result};
use aya::maps::{HashMap, MapData};
use log::info;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};

use backends::backends_server::BackendsServer;
use common::{BackendKey, BackendList, ClientKey, LoadBalancerMapping};
use config::TLSConfig;

pub async fn start(
    addr: Ipv4Addr,
    port: u16,
    backends_map: HashMap<MapData, BackendKey, BackendList>,
    gateway_indexes_map: HashMap<MapData, BackendKey, u16>,
    tcp_conns_map: HashMap<MapData, ClientKey, LoadBalancerMapping>,
    tls_config: TLSConfig,
) -> Result<()> {
    let (_, health_service) = tonic_health::server::health_reporter();

    tls_config.validate()?;

    let server = server::BackendService::new(backends_map, gateway_indexes_map, tcp_conns_map);
    let mut server_builder = Server::builder();
    server_builder = setup_tls(server_builder, &tls_config)?;
    server_builder
        .add_service(health_service)
        .add_service(BackendsServer::new(server))
        .serve(SocketAddrV4::new(addr, port).into())
        .await?;
    Ok(())
}

pub fn setup_tls(mut builder: Server, tls_config: &TLSConfig) -> Result<Server> {
    // TLS implementation drawn from Tonic examples.
    // See: https://github.com/hyperium/tonic/blob/master/examples/src/tls_client_auth/server.rs
    if tls_config.enable_tls || tls_config.enable_mtls {
        let mut tls = ServerTlsConfig::new();

        if let (Some(cert_path), Some(key_path)) = (
            &tls_config.server_certificate_path,
            &tls_config.server_private_key_path,
        ) {
            let cert = fs::read_to_string(cert_path)
                .with_context(|| format!("Failed to read certificate from {:?}", cert_path))?;
            let key = fs::read_to_string(key_path)
                .with_context(|| format!("Failed to read key from {:?}", key_path))?;
            let server_identity = Identity::from_pem(cert, key);
            tls = tls.identity(server_identity);
        }

        if tls_config.enable_mtls {
            if let Some(client_ca_cert_path) = &tls_config.client_certificate_authority_root_path {
                let client_ca_cert =
                    fs::read_to_string(client_ca_cert_path).with_context(|| {
                        format!("Failed to read client CA from {:?}", client_ca_cert_path)
                    })?;
                let client_ca_root = Certificate::from_pem(client_ca_cert);
                tls = tls.client_ca_root(client_ca_root);

                builder = builder.tls_config(tls)?;
                info!("gRPC mTLS enabled");
                return Ok(builder);
            }
        }
        builder = builder.tls_config(tls)?;
        info!("gRPC TLS enabled");
    }
    Ok(builder)
}
