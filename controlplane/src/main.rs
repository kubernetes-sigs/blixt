/*
Copyright 2024 The Kubernetes Authors.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

use controlplane::*;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use kube::Client;
use tokio::task::JoinHandle;
use tokio::try_join;
use tonic::transport::Server;
use tracing::{debug, error};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    run().await;
    Ok(())
}

pub async fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_file(true)
        .with_line_number(true)
        .init();

    let client = Client::try_default()
        .await
        .expect("failed to create kube Client");
    let ctx = Context {
        client: client.clone(),
    };

    // TODO: when TCPRoute and UDPRoute support is implemented
    //
    // use std::sync::Arc;
    // use controlplane::client_manager::DataplaneClientManager;
    // let dataplane_manager = Arc::new(DataplaneClientManager::new());

    if let Err(error) = try_join!(
        gateway_controller(ctx.clone()),
        gatewayclass_controller(ctx),
        setup_health_checks(IpAddr::from(Ipv4Addr::new(0, 0, 0, 0)), 8080)
    ) {
        error!("failed to start controllers: {error:?}");
        std::process::exit(1);
    }
}

// TODO: integrate with DataplaneClientManager connection status
// only get healthy once the dataplane pod connections are established
async fn setup_health_checks(addr: IpAddr, port: u16) -> Result<JoinHandle<()>> {
    let healthchecks = tokio::spawn(async move {
        let (_, health_service) = tonic_health::server::health_reporter();
        let server_builder = Server::builder();

        // by convention we add 1 to the API listen port and use that
        // for the health check port.
        let port = port + 1;

        let addr = match addr {
            IpAddr::V4(v4) => SocketAddr::V4(SocketAddrV4::new(v4, port)),
            IpAddr::V6(v6) => SocketAddr::V6(SocketAddrV6::new(v6, port, 0, 0)),
        };

        let server = server_builder.serve(addr, health_service);

        debug!("gRPC Health Checking service listens on {addr}");
        server
            .await
            .expect("Failed to serve gRPC Health Checking service");
    });
    Ok(healthchecks)
}
