use kube::Api;
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4};
use std::time::Duration;
use std::{env, io};

use controlplane::{K8sError, NamespacedName, controllers};
use gateway_api::apis::standard::gateways::Gateway;
use tests::infrastructure::{
    ContainerImages, ContainerRuntime, ImageAction, KindCluster, KustomizeDeployments, Workload,
    cargo_workspace_dir, default_containers,
};
use tests::{Error, Result, TestMode};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tracing::{debug, info};
use tracing_subscriber::EnvFilter;

#[tokio::test]
async fn integration_tcp_route() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_file(true)
        .with_line_number(true)
        .init();

    let cluster = KindCluster::new(
        "blixt-dev-integration-tcproute",
        ContainerRuntime::default(),
    )?;
    // TODO: the test is not successful on environments where a controlplane or dataplane is already
    // successfully running; ensure this cluster is fresh
    // for further details please refer to the FIXME in tests::deployments::default_containers
    match cluster.delete().await {
        Ok(_) => (),
        Err(e) => {
            debug!("failed to delete cluster {} error {e:?}", cluster.name())
        }
    }
    let images = prepare_default_cluster(&cluster, "integration-test").await?;

    let tcp_route_deployment = vec!["../config/samples/tcproute"];
    let deployments = KustomizeDeployments::new(cluster.clone(), tcp_route_deployment)?;
    deployments.apply().await?;

    // patches the workload images and runs a rollout restart & status
    // this is the first time the containers launch, as the default deployment is on "latest" tag
    let workloads = images.get_workloads();
    cluster
        .rollouts(&workloads, Some(Duration::from_secs(60)))
        .await?;

    let k8s_client = cluster.k8s_client().await?;
    let tcp_route_namespace = "blixt-samples-tcproute";
    let tcp_route_basename = "blixt-tcproute-sample";
    let tcp_route_port = 8080;

    let gateway_api: Api<Gateway> = Api::namespaced(k8s_client, tcp_route_namespace);
    let mut gateway = gateway_api
        .get(tcp_route_basename)
        .await
        .map_err(K8sError::client)?;

    let mut retry = 0;
    let mut gateway_ips = vec![];
    while retry < 30 {
        let ips = controllers::gateway::get_gateway_ips(&gateway).ok();
        if ips.is_some() {
            gateway_ips = ips.unwrap();
            break;
        };
        info!("Gateway IP not ready. Retrying ...");
        tokio::time::sleep(Duration::from_secs(3)).await;
        gateway = gateway_api
            .get(tcp_route_basename)
            .await
            .map_err(K8sError::client)?;
        retry += 1;
    }
    assert_eq!(
        1,
        gateway_ips.len(),
        "Timed out during locating the Gateway IP"
    );

    // TODO: multiple gateways and IPv6 support
    let gw_ip: Ipv4Addr = match gateway_ips[0] {
        IpAddr::V4(v4) => v4,
        IpAddr::V6(_) => {
            panic!("IPv6 not supported")
        }
    };

    info!("connecting via TCP to {} {}", gw_ip, tcp_route_port);
    let mut stream = TcpStream::connect(SocketAddrV4::new(gw_ip, tcp_route_port)).await?;
    info!("connected with local address: {}", stream.local_addr()?);
    stream.writable().await?;
    stream.write_all(b"yabba dabba doo\n").await?;
    info!("data was written to stream");

    let mut msg = vec![0; 1024];
    loop {
        info!("waiting for readable");
        stream.readable().await?;
        match stream.try_read(&mut msg) {
            Ok(n) => {
                info!("read");
                msg.truncate(n);
                break;
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }

    assert_eq!(&msg, b"echo-blixt-tcproute-sample yabba dabba doo\n");
    Ok(())
}

async fn prepare_default_cluster(
    cluster: &KindCluster,
    image_tag: &str,
) -> Result<ContainerImages> {
    cluster.start().await?;

    let images = default_images(cluster.clone(), image_tag)?;
    images.process().await?;

    let gateway_api_metallb = vec![
        "https://github.com/metallb/metallb/config/native?ref=v0.13.11",
        "https://github.com/kubernetes-sigs/gateway-api/config/crd/experimental?ref=v1.2.1",
    ];

    let deployments = KustomizeDeployments::new(cluster.clone(), gateway_api_metallb)?;
    deployments.apply().await?;

    cluster
        .rollout_status(
            Workload::Deployment(NamespacedName {
                name: "controller".to_string(),
                namespace: "metallb-system".to_string(),
            }),
            Duration::from_secs(60),
        )
        .await?;

    let deployments = vec!["../config/metallb/", "../config/default/"];
    let deployments = KustomizeDeployments::new(cluster.clone(), deployments)?;
    deployments.apply().await?;
    Ok(images)
}

/// builds and loads controlplane, dataplane, udp_test_server
fn default_images(cluster: KindCluster, image_tag: &str) -> Result<ContainerImages, Error> {
    let cargo_workspace_dir = cargo_workspace_dir("tests")?;
    let images = ContainerImages {
        cargo_workspace_dir: cargo_workspace_dir.clone(),
        container_runtime: ContainerRuntime::default(),
        container_host: env::var("CONTAINER_HOST").ok(),
        containers: default_containers(TestMode::Development, &cargo_workspace_dir)?,
        action: ImageAction::default(),
        tag: image_tag.to_string(),
        registry: Some("ghcr.io/kubernetes-sigs".to_string()),
        kind_cluster: Some(cluster.clone()),
    };
    Ok(images)
}
