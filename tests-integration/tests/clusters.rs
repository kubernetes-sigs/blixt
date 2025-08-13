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
use std::env;
use std::time::Duration;

use k8s_openapi::api::apps::v1::Deployment;
use kube::Api;
use tests_integration::Result;
use tests_integration::infrastructure::{
    ImageTag, KindCluster, KustomizeDeployments, NamespacedName, Workload, WorkloadImageTag,
};
use tracing_subscriber::EnvFilter;

async fn create_cluster() -> Result<KindCluster> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_file(true)
        .with_line_number(true)
        .init();

    let cluster = KindCluster::new("blixt-tests-integration")?;
    cluster.create().await?;

    let tag = env::var("TAG").unwrap_or("integration-tests".to_string());
    let registry = env::var("REGISTRY").unwrap_or("ghcr.io/kubernetes-sigs".to_string());
    let controlplane_image =
        env::var("BLIXT_CONTROLPLANE_IMAGE").unwrap_or(format!("{registry}/blixt-controlplane"));
    let dataplane_image =
        env::var("BLIXT_DATAPLANE_IMAGE").unwrap_or(format!("{registry}/blixt-dataplane"));
    let udp_server_image =
        env::var("BLIXT_UDP_SERVER_IMAGE").unwrap_or(format!("{registry}/blixt-udp-test-server"));

    cluster.load_image(&controlplane_image, &tag).await?;
    cluster.load_image(&dataplane_image, &tag).await?;
    cluster.load_image(&udp_server_image, &tag).await?;

    let gateway_api_metallb = vec![
        "https://github.com/metallb/metallb/config/native?ref=v0.15.2",
        "https://github.com/kubernetes-sigs/gateway-api/config/crd/experimental?ref=v1.3.0",
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

    let deployments = vec!["../config/tests/metallb/", "../config/default/"];
    let deployments = KustomizeDeployments::new(cluster.clone(), deployments).await?;
    deployments.apply().await?;

    // patches the workload images and runs a rollout restart & status
    // this is the first time the containers launch, as the default deployment is on "latest" tag
    cluster
        .rollouts(
            &[
                WorkloadImageTag {
                    id: Workload::DaemonSet(NamespacedName {
                        namespace: "blixt-system".to_string(),
                        name: "blixt-dataplane".to_string(),
                    }),
                    image_tag: Some(ImageTag {
                        image: dataplane_image,
                        tag: tag.clone(),
                    }),
                },
                WorkloadImageTag {
                    id: Workload::Deployment(NamespacedName {
                        namespace: "blixt-system".to_string(),
                        name: "blixt-controlplane".to_string(),
                    }),
                    image_tag: Some(ImageTag {
                        image: controlplane_image.clone(),
                        tag: tag.clone(),
                    }),
                },
            ],
            Some(Duration::from_secs(60)),
        )
        .await?;

    // test k8s client connection
    let blixt_namespace = "blixt-system";
    let controlplane_deployment = "blixt-controlplane";

    let k8s_client = cluster.k8s_client().await?;
    let deployment_api: Api<Deployment> = Api::namespaced(k8s_client, blixt_namespace);
    let controlplane_deployment = deployment_api.get(controlplane_deployment).await;
    assert!(controlplane_deployment.is_ok());

    // check if image tag on deployment is correct
    let deployment = controlplane_deployment.unwrap();
    let spec = deployment.spec.unwrap();
    let pod = spec.template.spec.unwrap();
    assert!(
        pod.containers
            .iter()
            .any(|c| if let Some(image) = &c.image {
                image == &format!("{controlplane_image}:{tag}")
            } else {
                false
            })
    );

    Ok(cluster)
}

#[tokio::test]
async fn cluster_test() -> Result<()> {
    let cluster = create_cluster().await?;
    cluster.delete().await?;
    Ok(())
}
