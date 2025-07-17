use crate::Result;
use crate::infrastructure::{ContainerRuntime, ContainerState};
use kube::config::KubeConfigOptions;
use kube::{Client, Config};
use thiserror::Error as ThisError;
use xshell::{Shell, cmd};

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

impl KindCluster {
    pub fn new(name: &str, runtime: ContainerRuntime) -> Result<Self> {
        Ok(KindCluster {
            name: name.to_string(),
            role: "control-plane".to_string(),
            sh: Shell::new().map_err(|e| KindError::Execution(e.to_string()))?,
            runtime,
        })
    }

    pub fn k8s_context(&self) -> String {
        format!("kind-{}", self.name)
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn state(&self) -> Result<ContainerState> {
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

    pub fn ready(&self) -> Result<()> {
        let state = self.state()?;
        match &state {
            ContainerState::Running => Ok(()),
            ContainerState::Exited | ContainerState::NotFound => {
                Err(KindError::ContainerState(self.name.to_string(), state.clone()).into())
            }
        }
    }

    pub fn start(&self) -> Result<()> {
        let name = self.name.as_str();
        match self.state()? {
            ContainerState::Running => Ok(()),
            ContainerState::Exited => {
                let container_id = self.container_id()?;
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

    pub fn delete(&self) -> Result<()> {
        let runtime = self.runtime.to_string();
        let container_id = self.container_id()?;
        cmd!(self.sh, "{runtime} stop {container_id}")
            .run()
            .map_err(|e| KindError::Execution(e.to_string()))?;

        cmd!(self.sh, "{runtime} rm {container_id}")
            .run()
            .map_err(|e| KindError::Execution(e.to_string()).into())
    }

    pub fn container_id(&self) -> Result<String> {
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

    pub async fn get_client(self) -> Result<Client> {
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
}
