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

use std::path::{Path, PathBuf};

use thiserror::Error as ThisError;
use tracing::error;

use crate::Result;
use crate::infrastructure::{AsyncCommand, AsyncCommandError, KindCluster, KindClusterError};

/// Errors originating from [`KustomizeDeployments`].
#[allow(missing_docs)]
#[derive(ThisError, Debug)]
pub enum KustomizeError {
    #[error("path {0} is invalid {1:?}")]
    InvalidPath(String, String),
    #[error("failed to render kustomize input {0:?}")]
    Render(AsyncCommandError),
    #[error("apply error: {0}")]
    Apply(AsyncCommandError),
    #[error(transparent)]
    Kind(#[from] KindClusterError),
}

/// Set of kustomize deployments linked to a cluster.
pub struct KustomizeDeployments {
    cluster: KindCluster,
    kustomizations: Vec<KustomizeKind>,
}

enum KustomizeKind {
    Directory(PathBuf),
    Https(String),
}

impl KustomizeDeployments {
    /// create a set of kustomize deployments
    pub async fn new<D: IntoIterator<Item = impl AsRef<str>>>(
        cluster: KindCluster,
        kustomizations: D,
    ) -> Result<Self> {
        let mut validated_kustomizations = vec![];
        for k in kustomizations.into_iter() {
            validated_kustomizations.push(KustomizeKind::try_from(k.as_ref()).await?);
        }

        Ok(Self {
            cluster,
            kustomizations: validated_kustomizations,
        })
    }

    /// apply the kustomize deployments on the provided cluster
    pub async fn apply(&self) -> Result<()> {
        let k8s_ctx = self.cluster.k8s_context();
        for deployment in &self.kustomizations {
            let inner = deployment.inner();
            AsyncCommand::new(
                "kubectl",
                &[
                    format!("--context={k8s_ctx}").as_str(),
                    "apply",
                    "--kustomize",
                    inner.as_str(),
                ],
            )
            .run()
            .await
            .map_err(KustomizeError::Apply)?;
        }

        Ok(())
    }
}

impl KustomizeKind {
    async fn try_from<K: AsRef<str>>(kustomization: K) -> Result<KustomizeKind> {
        let kustomization = kustomization.as_ref();

        let kind = match kustomization {
            _ if kustomization.starts_with("https://") => {
                KustomizeKind::Https(kustomization.to_string())
            }
            _ => {
                let path = kustomization.to_string();
                let fs_path = Path::new(&path);

                if !fs_path
                    .try_exists()
                    .map_err(|e| KustomizeError::InvalidPath(path.to_string(), e.to_string()))?
                {
                    return Err(KustomizeError::InvalidPath(
                        path.to_string(),
                        "does not exist".to_string(),
                    )
                    .into());
                }

                if !fs_path.is_dir() {
                    return Err(KustomizeError::InvalidPath(
                        path.to_string(),
                        "is a file, not a directory".to_string(),
                    )
                    .into());
                }

                KustomizeKind::Directory(fs_path.to_path_buf())
            }
        };

        kind.validate().await
    }

    async fn validate(self) -> Result<Self> {
        match &self {
            KustomizeKind::Directory(_) => {
                let inner = self.inner();
                AsyncCommand::new("kubectl", &["kustomize", inner.as_str()])
                    .run()
                    .await
                    .map_err(KustomizeError::Render)?;
            }
            KustomizeKind::Https(_) => { /* skipping, typically valid, and validation is relatively slow */
            }
        }

        Ok(self)
    }

    fn inner(&self) -> String {
        match self {
            KustomizeKind::Directory(d) => d.as_os_str().to_string_lossy().to_string(),
            KustomizeKind::Https(d) => d.clone(),
        }
    }
}
