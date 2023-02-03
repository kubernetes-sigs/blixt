pub mod backends;
pub mod netutils;
pub mod server;

use std::net::{Ipv4Addr, SocketAddrV4};

use anyhow::Error;
use aya::maps::{HashMap, MapRefMut};
use tonic::transport::Server;

use backends::backends_server::BackendsServer;
use common::{BackendsList, BackendKey, BackendsIndexes};

pub async fn start(
    addr: Ipv4Addr,
    port: u16,
    backends_map: HashMap<MapRefMut, BackendKey, BackendsList>,
    backends_indexes_map: HashMap<MapRefMut, BackendKey, BackendsIndexes>,
) -> Result<(), Error> {
    let server = server::BackendService::new(backends_map, backends_indexes_map);
    // TODO: mTLS https://github.com/Kong/blixt/issues/50
    Server::builder()
        .add_service(BackendsServer::new(server))
        .serve(SocketAddrV4::new(addr, port).into())
        .await?;
    Ok(())
}
