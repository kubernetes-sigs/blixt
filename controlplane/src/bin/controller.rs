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

use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use clap::Parser;
use kube::Client;
use tokio::task::JoinHandle;
use tokio::try_join;
use tonic::transport::Server;
use tracing::{debug, error, info};

use controlplane::Result;
use controlplane::consts::{BLIXT_APP_LABEL, BLIXT_DATAPLANE_COMPONENT_LABEL, BLIXT_NAMESPACE};
use controlplane::controllers::{GatewayClassController, GatewayController, TCPRouteController};
use controlplane::dataplane::DataplaneClientManager;

const BEFORE_HELP_MESSAGE: &str = "
Blixt Controlplane

Provides the required k8s controllers to configure the Blixt Controlplane through
GatewayApi CRDs like Gateway, GatewayClass, TCPRoute, ...";

#[derive(Debug, Parser)]
#[command(author, version, about, before_help = BEFORE_HELP_MESSAGE)]
pub struct Options {
    /// dataplane backend service GRPC port
    #[clap(default_value_t = 9874)]
    pub dataplane_service_port: u16,
    /// dataplane service namespace
    #[clap(default_value_t = BLIXT_NAMESPACE.to_string())]
    pub dataplane_service_namespace: String,
    /// dataplane service app label to locate dataplane pods
    #[clap(default_value_t = BLIXT_APP_LABEL.to_string())]
    pub dataplane_service_app_label: String,
    /// dataplane service component label to locate dataplane pods
    #[clap(default_value_t = BLIXT_DATAPLANE_COMPONENT_LABEL.to_string())]
    pub dataplane_service_component_label: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_file(true)
        .with_line_number(true)
        .init();

    let opts = Options::parse();
    info!("cli options: {:?}", opts);

    match run(&opts).await {
        Ok(()) => info!("Success."),
        Err(e) => {
            error!("{e:?}");
            std::process::exit(1)
        }
    }
}

pub async fn run(opts: &Options) -> Result<()> {
    let client = Client::try_default()
        .await
        .expect("failed to create kube Client");

    let dataplane_client = DataplaneClientManager::new(
        opts.dataplane_service_namespace.clone(),
        opts.dataplane_service_app_label.clone(),
        opts.dataplane_service_component_label.clone(),
        opts.dataplane_service_port,
    );

    // TODO: update clients on Node (add, remove) and Pod events (dataplane rollout)
    dataplane_client.update_clients(client.clone()).await?;

    let tcproute_controller = TCPRouteController::new(client.clone(), dataplane_client.clone());
    let gateway_controller = GatewayController::new(client.clone());
    let gatewayclass_controller = GatewayClassController::new(client.clone());

    if let Err(error) = try_join!(
        gateway_controller.start(),
        tcproute_controller.start(),
        gatewayclass_controller.start(),
        setup_health_checks(IpAddr::from(Ipv4Addr::new(0, 0, 0, 0)), 8080),
    ) {
        error!("failed to start controllers: {error:?}");
        std::process::exit(1);
    }

    Ok(())
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
