use kube::config::KubeConfigOptions;
use kube::{Client, Config};
use std::time::Duration;
use thiserror::Error as ThisError;
use tracing::info;
use xshell::{Shell, cmd};

use crate::Result;
use crate::infrastructure::{
    ContainerRuntime, ContainerState, ImageError, Workload, WorkloadImageTag,
};

#[derive(Clone, Debug)]
pub struct KindCluster {
    name: String,
    role: String,
    sh: Shell,
    runtime: ContainerRuntime,
}

#[derive(ThisError, Debug)]
pub enum KindError {
    #[error("execution error: {0}")]
    Execution(String),
    #[error("{0} container state {1:?}")]
    ContainerState(String, ContainerState),
    #[error("failed to create client {1} for k8s context {0:?}")]
    Client(String, String),
}

// NOTE: public facing methods are having provisional async even this is at the moment not utilized
// this is likely useful when replacing the xshell/cmds (e.g. with api calls through async clients)
// using tokio::process::Command to allow spawning tasks for the processes could be another approach
// xshell is not thread safe, but it would be great to e.g. spawn tokio tasks for
// cluster build and image build in parallel
impl KindCluster {
    pub fn new(name: &str, runtime: ContainerRuntime) -> Result<Self> {
        Ok(KindCluster {
            name: name.to_string(),
            role: "control-plane".to_string(),
            sh: Shell::new().map_err(|e| KindError::Execution(e.to_string()))?,
            runtime,
        })
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn k8s_context(&self) -> String {
        format!("kind-{}", self.name)
    }

    pub async fn k8s_client(self) -> Result<Client> {
        let kube_config = KubeConfigOptions {
            context: Some(self.k8s_context()),
            cluster: None,
            user: None,
        };
        let cfg = Config::from_kubeconfig(&kube_config)
            .await
            .map_err(|e| KindError::Client(self.k8s_context(), e.to_string()))?;

        let client = Client::try_from(cfg)
            .map_err(|e| KindError::Client(self.k8s_context(), e.to_string()))?;

        Ok(client)
    }

    pub async fn container_id(&self) -> Result<String> {
        let runtime = self.runtime.to_string();
        let filter = format!(r#"name=^{}-{}$"#, self.name, self.role);
        let output = cmd!(self.sh, "{runtime} ps --no-trunc -q --filter {filter}")
            .read()
            .map_err(|e| KindError::Execution(e.to_string()))?;

        match output.len() == 64 {
            true => Ok(output),
            false => Err(KindError::Execution(format!(
                "Could not determine container id for {}",
                self.name
            ))
            .into()),
        }
    }

    pub async fn start(&self) -> Result<()> {
        let name = self.name.as_str();
        match self.state().await? {
            ContainerState::Running => Ok(()),
            ContainerState::Exited => {
                let container_id = self.container_id().await?;
                let runtime = self.runtime.to_string();
                cmd!(self.sh, "{runtime} start {container_id}")
                    .run()
                    .map_err(|e| KindError::Execution(e.to_string()).into())
                    .map(|_| ())
            }
            ContainerState::NotFound => cmd!(self.sh, "kind create cluster --name {name}")
                .run()
                .map_err(|e| KindError::Execution(e.to_string()).into())
                .map(|_| ()),
        }
    }

    pub async fn delete(&self) -> Result<()> {
        let runtime = self.runtime.to_string();
        let container_id = self.container_id().await?;
        cmd!(self.sh, "{runtime} stop {container_id}")
            .run()
            .map_err(|e| KindError::Execution(e.to_string()))?;

        cmd!(self.sh, "{runtime} rm {container_id}")
            .run()
            .map_err(|e| KindError::Execution(e.to_string()).into())
    }

    pub async fn state(&self) -> Result<ContainerState> {
        let filter = format!(r#"name=^{}-{}$"#, self.name, self.role);
        let format = r#"'{{.State}}'"#;
        let runtime = self.runtime.to_string();

        let output = cmd!(
            self.sh,
            "{runtime} ps --no-trunc --format {format} --filter {filter}"
        )
        .read()
        .map_err(|e| KindError::Execution(e.to_string()))?;

        Ok(match output.as_str() {
            r#"'running'"# => ContainerState::Running,
            r#"'exited'"# => ContainerState::Exited,
            _ => ContainerState::NotFound,
        })
    }

    pub async fn ready(&self) -> Result<()> {
        let state = self.state().await?;
        match &state {
            ContainerState::Running => Ok(()),
            ContainerState::Exited | ContainerState::NotFound => {
                Err(KindError::ContainerState(self.name.to_string(), state.clone()).into())
            }
        }
    }

    pub async fn load_image(&self, image: &str, tag: &str) -> Result<()> {
        let kind_cluster = &self.name;
        info!("Loading image {image} with {tag} to kind cluster {kind_cluster:?}.");
        cmd!(
            self.sh,
            "kind load docker-image {image}:{tag} --name {kind_cluster}"
        )
        .run()
        .map_err(|_| ImageError::Load(image.to_string(), tag.to_string()).into())
        .map(|_| ())
    }

    /// In case wait_status is None the rollouts are not waiting for successful
    /// In case wait_status is Some(Duration) the rollouts wait for success with
    /// the duration as timeout per rollout
    pub async fn rollout<T: AsRef<WorkloadImageTag>>(
        &self,
        workload: T,
        wait_status: Option<Duration>,
    ) -> Result<()> {
        let workload = workload.as_ref();
        let k8s_ctx = self.k8s_context();

        let (workload_type, namespace, name) = workload.workload_namespace_name();

        // update deployment image references in case specified
        if let Some((image, tag)) = workload.image_tag() {
            info!(
                "Updating image {image} with tag {tag} for rollout {}.",
                workload.id
            );

            cmd!(
            self.sh,
            "kubectl --context={k8s_ctx} set image -n {namespace} {workload_type}/{name} *={image}:{tag}"
        )
                .run()
                .map_err(|e| KindError::Execution(e.to_string()))?;
        }

        info!("Restarting rollout {}.", workload.id);
        cmd!(
            self.sh,
            "kubectl --context={k8s_ctx} rollout restart -n {namespace} {workload_type}/{name}"
        )
        .run()
        .map_err(|e| KindError::Execution(e.to_string()))?;

        if let Some(wait_status) = wait_status {
            self.rollout_status(&workload.id, wait_status).await?;
        };

        Ok(())
    }

    pub async fn rollouts<T: AsRef<WorkloadImageTag>>(
        &self,
        workloads: &[T],
        wait_status: Option<Duration>,
    ) -> Result<()> {
        for workload in workloads {
            self.rollout(workload, wait_status).await?;
        }
        Ok(())
    }

    pub async fn rollout_status<T: AsRef<Workload>>(
        &self,
        workload: T,
        timeout_secs: Duration,
    ) -> Result<()> {
        let k8s_ctx = self.k8s_context();
        let timeout = timeout_secs.as_secs().to_string();
        let workload = workload.as_ref();

        info!(
            "Waiting for rollout {} to complete. Timeout: {}",
            workload, timeout
        );
        let (workload_type, namespace, name) = workload.workload_namespace_name();
        cmd!(
            self.sh,
            "kubectl --context={k8s_ctx} rollout status -n {namespace} {workload_type}/{name} --timeout {timeout}s"
        )
            .run()
            .map_err(|e| KindError::Execution(e.to_string()).into())
    }
}
