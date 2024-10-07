/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/
use anyhow::{anyhow, Result};
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser, Clone)]
pub struct TLSConfig {
    #[clap(long, default_value = "false")]
    pub enable_tls: bool,
    #[clap(long, default_value = "false")]
    pub enable_mtls: bool,
    pub server_certificate_path: Option<PathBuf>,
    pub server_private_key_path: Option<PathBuf>,
    pub client_certificate_authority_root_path: Option<PathBuf>,
}

impl TLSConfig {
    pub fn validate(&self) -> Result<()> {
        let tls_paths = &[
            (&self.server_certificate_path, "server_certificate_path"),
            (&self.server_private_key_path, "server_private_key_path"),
        ];

        let mtls_paths = &[(
            &self.client_certificate_authority_root_path,
            "client_certificate_authority_root_path",
        )];

        if self.enable_mtls {
            validate_paths(tls_paths)?;
            validate_paths(mtls_paths)?;
        } else if self.enable_tls {
            validate_paths(tls_paths)?;
        }

        Ok(())
    }
}

fn validate_paths(paths: &[(&Option<PathBuf>, &str)]) -> Result<()> {
    for (path, name) in paths {
        if path.is_none() {
            return Err(anyhow!("Missing required path: {}", name));
        }
    }
    Ok(())
}
