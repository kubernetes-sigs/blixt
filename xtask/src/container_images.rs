use std::env;
use std::path::PathBuf;

use anyhow::anyhow;
use clap::{Parser, ValueEnum};
use tests::TestMode;
use tests::infrastructure::{
    self, ContainerImages, KindCluster, cargo_workspace_dir, default_containers,
};
use tracing::info;

#[derive(Debug, Parser)]
pub struct CliArgs {
    #[clap(default_value = "latest")]
    pub tag: String,

    #[clap(value_enum, default_value_t = ImageAction::Start)]
    pub action: ImageAction,

    #[clap(default_value = "ghcr.io/kubernetes-sigs")]
    pub registry: String,

    #[clap(default_value = "build/develop")]
    pub containerfile_directory: PathBuf,

    #[clap(value_enum, default_value_t = ContainerRuntime::Podman)]
    pub container_runtime: ContainerRuntime,

    #[clap(default_value = "blixt-dev")]
    pub kind_cluster: String,
}

#[derive(ValueEnum, Default, Debug, Clone)]
pub enum ImageAction {
    Build,
    #[default]
    Load,
    Start,
}

#[derive(ValueEnum, Default, Debug, Clone)]
pub enum ContainerRuntime {
    #[default]
    Podman,
}

pub async fn run(opts: CliArgs) -> Result<(), anyhow::Error> {
    info!("{:?}", opts);

    let cargo_workspace_dir = cargo_workspace_dir("xtask")?;
    let images = ContainerImages {
        cargo_workspace_dir: cargo_workspace_dir.clone(),
        container_runtime: opts.container_runtime.clone().into(),
        container_host: env::var("CONTAINER_HOST").ok(),
        containers: default_containers(TestMode::Development, &cargo_workspace_dir)?,
        action: opts.action.into(),
        tag: opts.tag,
        registry: Some(opts.registry),
        kind_cluster: Some(KindCluster::new(
            &opts.kind_cluster,
            opts.container_runtime.into(),
        )?),
    };

    images.process().await.map_err(|e| anyhow!("{}", e))
}

impl From<ImageAction> for infrastructure::ImageAction {
    fn from(value: ImageAction) -> Self {
        match value {
            ImageAction::Build => infrastructure::ImageAction::Build,
            ImageAction::Load => infrastructure::ImageAction::Load,
            ImageAction::Start => infrastructure::ImageAction::Rollout,
        }
    }
}

impl From<ContainerRuntime> for infrastructure::ContainerRuntime {
    fn from(value: ContainerRuntime) -> Self {
        match value {
            ContainerRuntime::Podman => infrastructure::ContainerRuntime::Podman,
        }
    }
}
