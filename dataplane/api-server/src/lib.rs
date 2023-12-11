/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

pub mod backends;
pub mod netutils;
pub mod server;

use std::net::{Ipv4Addr, SocketAddrV4};

use anyhow::Error;
use aya::maps::{HashMap, MapData};
use tonic::transport::Server;

use backends::backends_server::BackendsServer;
use common::{BackendKey, BackendList, ClientKey, TCPBackend};

pub async fn start(
    addr: Ipv4Addr,
    port: u16,
    backends_map: HashMap<MapData, BackendKey, BackendList>,
    gateway_indexes_map: HashMap<MapData, BackendKey, u16>,
    tcp_conns_map: HashMap<MapData, ClientKey, TCPBackend>,
) -> Result<(), Error> {
    let server = server::BackendService::new(backends_map, gateway_indexes_map, tcp_conns_map);
    // TODO: mTLS https://github.com/Kong/blixt/issues/50
    Server::builder()
        .add_service(BackendsServer::new(server))
        .serve(SocketAddrV4::new(addr, port).into())
        .await?;
    Ok(())
}
