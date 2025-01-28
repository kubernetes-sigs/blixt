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
use log::{debug, info, error};
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
    tls_config: Option<TLSConfig>,
) -> Result<()> {
    debug!("starting api server on {}", addr);

    // Tonic itself doesn't provide a built-in mechanism for selectively
    // applying TLS based on routes, as TLS configuration is tied to the
    // entire server and managed at the transport layer, not at the
    // application layer where routes are defined.
    //
    // Solution: separate gRPC services
    //
    // Public server without TLS (healthchecks ONLY)
    let healthchecks = tokio::spawn(async move {
        let (_, health_service) = tonic_health::server::health_reporter();
        let mut server_builder = Server::builder();

        // by convention we add 1 to the API listen port and use that
        // for the health check port.
        let port = port + 1;
        let addr = SocketAddrV4::new(addr, port);
        let server = server_builder
            .add_service(health_service)
            .serve(addr.into());

        debug!("gRPC Health Checking service listens on port {}", port);
        server.await.map_err(|e| {
            error!("Failed serve gRPC Health Checking service, err: {:?}", e);
            e
        }).unwrap();
    });

    // Secure server with (optional) mTLS
    let backends = tokio::spawn(async move {
        let server = server::BackendService::new(backends_map, gateway_indexes_map, tcp_conns_map);

        let mut server_builder = Server::builder();
        server_builder = setup_tls(server_builder, &tls_config).unwrap();

        let tls_addr = SocketAddrV4::new(addr, port);
        let tls_server = server_builder
            .add_service(BackendsServer::new(server))
            .serve(tls_addr.into());

        debug!("TLS server listens on port {}", port);
        tls_server.await.map_err(|e| {
            error!("Failed to serve TLS, err: {:?}", e);
            e
        }).unwrap();
    });

    tokio::try_join!(healthchecks, backends)?;

    Ok(())
}

pub fn setup_tls(mut builder: Server, tls_config: &Option<TLSConfig>) -> Result<Server> {
    // TLS implementation drawn from Tonic examples.
    // See: https://github.com/hyperium/tonic/blob/master/examples/src/tls_client_auth/server.rs
    match tls_config {
        Some(TLSConfig::TLS(config)) => {
            let mut tls = ServerTlsConfig::new();

            let cert = fs::read_to_string(&config.server_certificate_path).with_context(|| {
                format!(
                    "Failed to read certificate from {:?}",
                    config.server_certificate_path
                )
            })?;
            let key = fs::read_to_string(&config.server_private_key_path).with_context(|| {
                format!(
                    "Failed to read key from {:?}",
                    config.server_private_key_path
                )
            })?;
            let server_identity = Identity::from_pem(cert, key);
            tls = tls.identity(server_identity);

            builder = builder.tls_config(tls)?;
            info!("gRPC TLS enabled");
            Ok(builder)
        }
        Some(TLSConfig::MutualTLS(config)) => {
            let mut tls = ServerTlsConfig::new();

            let cert =
                fs::read_to_string(config.server_certificate_path.clone()).with_context(|| {
                    format!(
                        "Failed to read certificate from {:?}",
                        config.server_certificate_path
                    )
                })?;
            let key =
                fs::read_to_string(config.server_private_key_path.clone()).with_context(|| {
                    format!(
                        "Failed to read key from {:?}",
                        config.server_private_key_path
                    )
                })?;
            let server_identity = Identity::from_pem(cert, key);
            tls = tls.identity(server_identity);

            let client_ca_cert =
                fs::read_to_string(config.client_certificate_authority_root_path.clone())
                    .with_context(|| {
                        format!(
                            "Failed to read client CA from {:?}",
                            config.client_certificate_authority_root_path
                        )
                    })?;
            let client_ca_root = Certificate::from_pem(client_ca_cert);
            tls = tls.client_ca_root(client_ca_root);

            builder = builder.tls_config(tls)?;
            info!("gRPC mTLS enabled");
            Ok(builder)
        }
        None => {
            info!("gRPC TLS is not enabled");
            Ok(builder)
        },
    }
}
