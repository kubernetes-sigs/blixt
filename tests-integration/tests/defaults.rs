/*
Copyright 2025 The Kubernetes Authors.

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

use std::time::Duration;

use tests_integration::Result;
use tests_integration::infrastructure::{
    Container, ContainerImageAction, ContainerImages, ContainerRuntime, KindCluster,
    KustomizeDeployments, NamespacedName, Workload,
};
use tests_integration::{Error, cargo_workspace_dir, verify_path};
use tokio::try_join;

/// starts the cluster, loads gateway api and metallb, deploys blixt default config
pub(crate) async fn prepare_cluster(
    cluster: &KindCluster,
    image_tag: &str,
) -> Result<ContainerImages> {
    let mut images = images(cluster, image_tag)?;
    try_join!(cluster.start(), images.process())?;

    images.load_images(cluster).await?;

    let gateway_api_metallb = vec![
        "https://github.com/metallb/metallb/config/native?ref=v0.13.11",
        "https://github.com/kubernetes-sigs/gateway-api/config/crd/experimental?ref=v1.2.1",
    ];

    let deployments = KustomizeDeployments::new(cluster.clone(), gateway_api_metallb).await?;
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
    let deployments = KustomizeDeployments::new(cluster.clone(), deployments).await?;
    deployments.apply().await?;

    // patches the workload images and runs a rollout restart & status
    // this is the first time the containers launch, as the default deployment is on "latest" tag
    let workloads = images.get_workloads();
    cluster
        .rollouts(&workloads, Some(Duration::from_secs(60)))
        .await?;

    Ok(images)
}

/// builds controlplane, dataplane and udp_test_server
pub(crate) fn images(cluster: &KindCluster, image_tag: &str) -> Result<ContainerImages, Error> {
    let cargo_workspace_dir = cargo_workspace_dir("/tests-integration")?;

    let images = ContainerImages {
        cargo_workspace_dir: cargo_workspace_dir.clone(),
        container_runtime: ContainerRuntime::default(),
        container_host: None,
        containers: containers(&cargo_workspace_dir)?,
        action: ContainerImageAction::Build,
        tag: image_tag.to_string(),
        registry: Some("ghcr.io/kubernetes-sigs".to_string()),
        kind_cluster: Some(cluster.clone()),
        pre_build_commands: vec![],
    };
    Ok(images)
}

pub(crate) fn containers(cargo_workspace_dir: &str) -> Result<Vec<Container>> {
    Ok(vec![
        {
            let app = "dataplane";
            Container {
                containerfile: verify_path(format!(
                    "{cargo_workspace_dir}/build/Containerfile.{app}"
                ))?,
                image_name: format!("blixt-{app}"),
                workload: Some(Workload::DaemonSet(NamespacedName {
                    namespace: "blixt-system".to_string(),
                    name: format!("blixt-{app}"),
                })),
            }
        },
        // FIXME: oder for containers MUST not affect results
        // currently if dataplane is restarted after controlplane the EBPF tables stay empty
        // as controlplane configures the old dataplane daemonset, ideally this is fixed on the EBFP level
        // by not initializing the according maps on startup if present on the system
        // needs to be checked if this is possible, but would allow for continuous operation during
        // dataplane rollouts
        {
            let app = "controlplane";
            Container {
                containerfile: verify_path(format!(
                    "{cargo_workspace_dir}/build/Containerfile.{app}"
                ))?,
                image_name: format!("blixt-{app}"),
                workload: Some(Workload::Deployment(NamespacedName {
                    namespace: "blixt-system".to_string(),
                    name: format!("blixt-{app}"),
                })),
            }
        },
        {
            let app = "udp_test_server";
            Container {
                containerfile: verify_path(format!(
                    "{cargo_workspace_dir}/build/Containerfile.{app}"
                ))?,
                image_name: format!("blixt-{app}"),
                workload: None,
            }
        },
    ])
}
