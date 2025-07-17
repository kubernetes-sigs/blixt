use crate::infrastructure::{KindCluster, KindError};
use crate::{Error, Result};
use std::path::{Path, PathBuf};
use thiserror::Error as ThisError;
use xshell::{Shell, cmd};

#[derive(ThisError, Debug)]
pub enum KustomizeError {
    #[error("path {0} does not exist {1:?}")]
    Path(String, std::io::Error),
    #[error("failed to render kustomize input")]
    Render(String),
    #[error("execution error: {0}")]
    Execution(String),
    #[error(transparent)]
    Kind(#[from] KindError),
}

pub struct KustomizeDeployments {
    cluster: KindCluster,
    kustomize_paths: Vec<KustomizeKind>,
    sh: Shell,
}

enum KustomizeKind {
    Directory(PathBuf),
    File(PathBuf),
    Https(String),
}

impl KustomizeKind {
    fn needs_k(&self) -> bool {
        match self {
            KustomizeKind::Directory(_) => true,
            KustomizeKind::File(_) => false,
            KustomizeKind::Https(_) => true,
        }
    }

    fn inner(&self) -> String {
        match self {
            KustomizeKind::Directory(d) => d.as_os_str().to_string_lossy().to_string(),
            KustomizeKind::File(d) => d.as_os_str().to_string_lossy().to_string(),
            KustomizeKind::Https(d) => d.clone(),
        }
    }
}

impl KustomizeDeployments {
    pub fn new(cluster: KindCluster, kustomizations: Vec<&str>) -> Result<Self> {
        Ok(Self {
            cluster,
            kustomize_paths: kustomizations
                .into_iter()
                .map(KustomizeKind::try_from)
                .collect::<Result<Vec<KustomizeKind>>>()?,
            sh: Shell::new().map_err(|e| KustomizeError::Execution(e.to_string()))?,
        })
    }

    pub async fn apply(&self) -> Result<()> {
        self.cluster.ready()?;

        let k8s_ctx = self.cluster.k8s_context();
        for deployment in &self.kustomize_paths {
            let inner = deployment.inner();
            match deployment.needs_k() {
                true => cmd!(self.sh, "kubectl --context={k8s_ctx} apply -k {inner}"),
                false => cmd!(self.sh, "kubectl --context={k8s_ctx} apply {inner}"),
            }
            .run()
            .map_err(|e| KustomizeError::Render(e.to_string()))?;
        }

        Ok(())
    }
}

impl TryFrom<&str> for KustomizeKind {
    type Error = Error;

    fn try_from(kustomization: &str) -> Result<Self> {
        let kind = match &kustomization {
            _ if kustomization.starts_with("https://") => {
                KustomizeKind::Https(kustomization.to_string())
            }
            _ => {
                let path = kustomization.to_string();
                let fs_path = Path::new(&path);
                fs_path
                    .try_exists()
                    .map_err(|e| KustomizeError::Path(path.to_string(), e))?;

                match fs_path.is_dir() {
                    true => KustomizeKind::Directory(fs_path.to_path_buf()),
                    false => KustomizeKind::File(fs_path.to_path_buf()),
                }
            }
        };

        kind.validate()
    }
}

impl KustomizeKind {
    fn validate(self) -> Result<Self> {
        let sh = Shell::new().map_err(|e| KustomizeError::Execution(e.to_string()))?;

        match &self {
            KustomizeKind::Directory(_) | KustomizeKind::File(_) => {
                let inner = self.inner();
                cmd!(sh, "kubectl kustomize {inner}")
                    .run()
                    .map_err(|e| KustomizeError::Render(e.to_string()))?;
            }
            KustomizeKind::Https(_) => { /* skipping, typically valid, and validation is relatively slow */
            }
        }

        Ok(self)
    }
}
