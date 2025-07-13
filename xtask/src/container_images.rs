use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

use anyhow::anyhow;
use clap::{Parser, ValueEnum};
use thiserror::Error;
use tracing::{debug, error, info, warn};
use xshell::{Shell, cmd};

#[derive(Debug, Parser)]
pub struct Options {
    #[clap(value_delimiter = ',', default_values_t = default_images(), long)]
    pub images: Vec<String>,

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

fn default_images() -> Vec<String> {
    ["controlplane", "dataplane", "udp_test_server"]
        .iter()
        .map(|app| format!("blixt-{app}:Containerfile.{app}"))
        .collect()
}

pub async fn run(opts: Options) -> Result<(), anyhow::Error> {
    debug!("{:?}", opts);

    let images = opts
        .images
        .iter()
        .filter_map(|i| {
            let s = i.split(':').collect::<Vec<&str>>();
            if s.len() != 2 {
                None
            } else {
                Some((s[0].to_string(), s[1].to_string()))
            }
        })
        .filter_map(|(image_name, container_file)| {
            let mut path = opts.containerfile_directory.clone();
            path.push(container_file);

            if path.exists() {
                Some((format!("{}/{image_name}", opts.registry), path))
            } else {
                warn!(
                    "Containerfile {:?} does not exist. Skipping...",
                    path.into_os_string()
                );
                None
            }
        })
        .collect::<HashMap<String, PathBuf>>();

    let Some(manifest_dir) = env::var("CARGO_MANIFEST_DIR").ok() else {
        return Err(anyhow!("Missing CARGO_MANIFEST_DIR value."));
    };
    let Some(manifest_dir) = manifest_dir.strip_suffix("/xtask") else {
        return Err(anyhow!("CARGO_MANIFEST_DIR does not end with /xtask"));
    };

    let images = Images {
        container_runtime: opts.container_runtime,
        action: opts.action,
        containers_files: images,
        container_host: env::var("CONTAINER_HOST").ok(),
        cargo_manifest_dir: manifest_dir.to_string(),
        tag: opts.tag,
        kind_cluster: opts.kind_cluster,
    };

    images.build().await
}

#[derive(Debug)]
struct Images {
    container_runtime: ContainerRuntime,
    container_host: Option<String>,
    containers_files: HashMap<String, PathBuf>,
    action: ImageAction,
    tag: String,
    kind_cluster: String,
    cargo_manifest_dir: String,
}

#[derive(Error, Debug)]
enum Error {
    #[error("Failed to create shell: {0}")]
    CouldNotCreateShell(xshell::Error),
    #[error("Failed to create shell: {0}")]
    CargoBuildFailed(xshell::Error),
    #[error("Failed to build image: {0}")]
    ImageBuildFailed(anyhow::Error),
    #[error("Failed to load image: {0}")]
    ImageLoadFailed(anyhow::Error),
    #[error("Failed to start image: {0}")]
    ImageStartFailed(anyhow::Error),
}

impl Images {
    async fn build(&self) -> Result<(), anyhow::Error> {
        let sh = match Shell::new() {
            Ok(sh) => sh,
            Err(e) => return Err(Error::CouldNotCreateShell(e).into()),
        };

        info!("running cargo build");
        cmd!(sh, "cargo build")
            .run()
            .map_err(Error::CargoBuildFailed)?;

        let _env_guard = if let Some(container_host) = &self.container_host {
            sh.push_env("CONTAINER_HOST", container_host)
        } else {
            sh.push_env("CONTAINER_HOST", "unix:///run/podman/podman.sock")
        };

        for (image, containerfile) in self.containers_files.iter() {
            if matches!(self.action, ImageAction::Build)
                || matches!(self.action, ImageAction::Load)
                || matches!(self.action, ImageAction::Start)
            {
                self.build_image(&sh, image, containerfile)
                    .map_err(Error::ImageBuildFailed)?;
            };

            if matches!(self.action, ImageAction::Load) || matches!(self.action, ImageAction::Start)
            {
                self.load_image(&sh, image)
                    .map_err(Error::ImageLoadFailed)?;
            };

            if matches!(self.action, ImageAction::Start) {
                self.start_image(&sh, image)
                    .map_err(Error::ImageStartFailed)?;
            }
        }

        Ok(())
    }

    fn build_image(
        &self,
        sh: &Shell,
        image: &str,
        containerfile: &PathBuf,
    ) -> Result<&String, anyhow::Error> {
        let container_runtime = format!("{:?}", self.container_runtime).to_lowercase();
        let manifest_dir = &self.cargo_manifest_dir;
        let volumes = format!("-v={manifest_dir}/:/workspace/");
        let tag = &self.tag;

        info!("building image {image:?} using containerfile {containerfile:?}");
        cmd!(
            sh,
            "{container_runtime} build --no-cache {volumes} --file {containerfile} -t {image}:{tag}"
        )
        .run()?;
        Ok(tag)
    }

    fn load_image(&self, sh: &Shell, image: &str) -> Result<(), anyhow::Error> {
        let tag = &self.tag;
        let kind_cluster = &self.kind_cluster;

        info!("loading image {image:?} to kind cluster {kind_cluster:?}");
        cmd!(
            sh,
            "kind load docker-image {image}:{tag} --name {kind_cluster}"
        )
        .run()?;
        Ok(())
    }

    fn start_image(&self, sh: &Shell, image: &str) -> Result<(), anyhow::Error> {
        info!("{image}");
        let image = *image.split(":").collect::<Vec<_>>().first().unwrap();
        let image = *image.split("/").collect::<Vec<_>>().last().unwrap();
        info!("{image}");
        let workload_deployment = match image {
            "blixt-controlplane" => Some(("deployment", "blixt-controlplane")),
            "blixt-dataplane" => Some(("daemonset", "blixt-dataplane")),
            _ => None,
        };

        if let Some((workload, deployment)) = workload_deployment {
            info!("restarting kubernetes {workload} {deployment}");
            cmd!(
                sh,
                "kubectl -n blixt-system rollout restart {workload} {deployment}"
            )
            .run()?
        };
        Ok(())
    }
}
