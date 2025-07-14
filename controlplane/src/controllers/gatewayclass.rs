/*
Copyright 2024 The Kubernetes Authors.

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

use futures::StreamExt;
use std::{
    ops::Sub,
    sync::Arc,
    time::{Duration, Instant},
};

use kube::{
    api::{Api, ListParams},
    runtime::{Controller, controller::Action, watcher::Config},
};

use crate::{consts::GATEWAY_CLASS_CONTROLLER_NAME, *};
use gateway_api::apis::standard::gatewayclasses::GatewayClass;
use gatewayclass_utils::*;
use tracing::{info, warn};

#[derive(Clone)]
pub struct GatewayClassController {
    k8s_client: Client,
}

impl GatewayClassController {
    pub fn new(k8s_client: Client) -> Self {
        Self { k8s_client }
    }

    pub async fn start(self) -> Result<()> {
        let gwc_api = Api::<GatewayClass>::all(self.k8s_client.clone());
        gwc_api
            .list(&ListParams::default().limit(1))
            .await
            .map_err(Error::CRDNotFoundError)?;

        Controller::new(gwc_api, Config::default().any_semantic())
            .shutdown_on_signal()
            .run(Self::reconcile, Self::error_policy, Arc::new(self))
            .filter_map(|x| async move { std::result::Result::ok(x) })
            .for_each(|_| futures::future::ready(()))
            .await;

        Ok(())
    }

    pub async fn reconcile(gateway_class: Arc<GatewayClass>, ctx: Arc<Self>) -> Result<Action> {
        let start = Instant::now();
        let name = gateway_class
            .metadata
            .name
            .clone()
            .ok_or(Error::InvalidConfigError(
                "no name provided for gatewayclass".to_string(),
            ))?;

        let mut gwc = GatewayClass {
            metadata: gateway_class.metadata.clone(),
            spec: gateway_class.spec.clone(),
            status: gateway_class.status.clone(),
        };

        if gateway_class.spec.controller_name != GATEWAY_CLASS_CONTROLLER_NAME {
            // Skip reconciling because we don't manage this resource
            return Ok(Action::await_change());
        }

        if !is_accepted(&gateway_class) {
            info!("marking gateway class {:?} as accepted", name);
            accept(&mut gwc);
            let gatewayclass_api = Api::<GatewayClass>::all(ctx.k8s_client.clone());
            patch_status(&gatewayclass_api, name, &gwc.status.unwrap_or_default()).await?;
        }

        let duration = Instant::now().sub(start);
        info!("finished reconciling in {:?} ms", duration.as_millis());
        Ok(Action::await_change())
    }

    fn error_policy(_: Arc<GatewayClass>, error: &Error, _: Arc<Self>) -> Action {
        warn!("reconcile failed: {:?}", error);
        Action::requeue(Duration::from_secs(5))
    }
}
