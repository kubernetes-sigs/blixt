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

use anyhow::Error;
use aya::maps::{HashMap, MapData};
use log::info;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};

use backends::backends_server::BackendsServer;
use common::{BackendKey, BackendList, ClientKey, LoadBalancerMapping};
use config::GrpcConfig;

pub async fn start(
    addr: Ipv4Addr,
    port: u16,
    backends_map: HashMap<MapData, BackendKey, BackendList>,
    gateway_indexes_map: HashMap<MapData, BackendKey, u16>,
    tcp_conns_map: HashMap<MapData, ClientKey, LoadBalancerMapping>,
    tls_config: GrpcConfig,
) -> Result<(), Error> {
    let (_, health_service) = tonic_health::server::health_reporter();

    let server = server::BackendService::new(backends_map, gateway_indexes_map, tcp_conns_map);
    let mut server_builder = Server::builder();
    server_builder = setup_tls(server_builder, &tls_config);
    server_builder
        .add_service(health_service)
        .add_service(BackendsServer::new(server))
        .serve(SocketAddrV4::new(addr, port).into())
        .await?;
    Ok(())
}

fn setup_tls(mut builder: Server, tls_config: &GrpcConfig) -> Server {
    if tls_config.enable_tls || tls_config.enable_mtls {
        let mut tls = ServerTlsConfig::new();

        if let Some(cert_path) = &tls_config.server_certificate_path {
            let cert = fs::read_to_string(cert_path).expect("Error reading server certificate");
            let key = fs::read_to_string(
                tls_config
                    .server_private_key_path
                    .as_ref()
                    .expect("Missing private key path"),
            )
            .expect("Error reading server private key");
            let server_identity = Identity::from_pem(cert, key);
            tls = tls.identity(server_identity);
        }

        if tls_config.enable_mtls {
            if let Some(client_ca_cert_path) = &tls_config.client_certificate_authority_root_path {
                let client_ca_cert =
                    fs::read_to_string(client_ca_cert_path).expect("Error reading CA certificate");
                let client_ca_root = Certificate::from_pem(client_ca_cert);
                tls = tls.client_ca_root(client_ca_root);

                builder = builder
                    .tls_config(tls)
                    .expect("Error adding mTLS to server");
                info!("gRPC mTLS enabled");
                return builder;
            }
        }
        builder = builder.tls_config(tls).expect("Error adding TLS to server");
        info!("gRPC TLS enabled");
    }
    builder
}
