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

mod defaults;

use std::time::Duration;

use defaults::prepare_cluster;
use k8s_openapi::api::apps::v1::Deployment;
use kube::Api;
use tests_integration::Result;
use tests_integration::infrastructure::{ContainerRuntime, ContainerState, KindCluster};
use tracing_subscriber::EnvFilter;

#[tokio::test]
async fn integration_cluster_images() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_file(true)
        .with_line_number(true)
        .init();

    let cluster = KindCluster::new("blixt-dev-cluster-images", ContainerRuntime::default())?;

    // build images, load and rollout
    let images = prepare_cluster(&cluster, "integration-release").await?;
    let workloads = images.get_workloads();
    cluster
        .rollouts(&workloads, Some(Duration::from_secs(60)))
        .await?;

    // test client connection
    let blixt_namespace = "blixt-system";
    let controlplane_deployment = "blixt-controlplane";

    let k8s_client = cluster.k8s_client().await?;
    let deployment_api: Api<Deployment> = Api::namespaced(k8s_client, blixt_namespace);
    let controlplane_deployment = deployment_api.get(controlplane_deployment).await;
    assert!(controlplane_deployment.is_ok());

    // check if image tag on deployment is correct
    let deployment = controlplane_deployment.unwrap();
    if let Some(spec) = deployment.spec {
        if let Some(pod) = spec.template.spec {
            assert!(
                pod.containers
                    .iter()
                    .any(|c| if let Some(image) = &c.image {
                        return image.ends_with("blixt-controlplane:integration-release");
                    } else {
                        false
                    })
            )
        } else {
            assert!(false)
        };
    } else {
        assert!(false)
    };

    // test deleting cluster
    cluster.delete().await?;
    assert_eq!(cluster.state().await?, ContainerState::NotFound);
    Ok(())
}
