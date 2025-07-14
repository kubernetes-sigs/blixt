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

use std::{
    ops::Sub,
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::Utc;
use futures::StreamExt;
use gateway_api::apis::standard::gatewayclasses::GatewayClass;
use gateway_api::constants::{GatewayConditionReason, GatewayConditionType};
use gateway_api::gatewayclasses::GatewayClassStatus;

use k8s_openapi::apimachinery::pkg::apis::meta::v1 as metav1;
use kube::api::{Patch, PatchParams};
use kube::{
    api::{Api, ListParams},
    runtime::{Controller, controller::Action, watcher::Config},
};
use serde_json::json;
use tracing::{info, warn};

use crate::consts::BLIXT_FIELD_MANAGER;
use crate::controllers::NamespaceName;
use crate::utils::set_condition;
use crate::{consts::GATEWAY_CLASS_CONTROLLER_NAME, *};

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
            .map_err(K8sError::Client)?; // TODO: map not found

        Controller::new(gwc_api, Config::default().any_semantic())
            .shutdown_on_signal()
            .run(Self::reconcile, Self::error_policy, Arc::new(self))
            .filter_map(|x| async move { std::result::Result::ok(x) })
            .for_each(|_| futures::future::ready(()))
            .await;

        Ok(())
    }

    async fn reconcile(gateway_class: Arc<GatewayClass>, ctx: Arc<Self>) -> Result<Action> {
        let start = Instant::now();
        let name = gateway_class.metadata.name()?;

        let mut gwc = gateway_class.as_ref().clone();
        if gateway_class.spec.controller_name != GATEWAY_CLASS_CONTROLLER_NAME {
            // Skip reconciling because we don't manage this resource
            return Ok(Action::await_change());
        }

        if !check_accepted(&gateway_class) {
            info!("marking gateway class {:?} as accepted", name);
            mark_accepted(&mut gwc);
            ctx.patch_status(name, &gwc.status.unwrap_or_default())
                .await?;
        }

        let duration = Instant::now().sub(start);
        info!("finished reconciling in {:?} ms", duration.as_millis());
        Ok(Action::await_change())
    }

    fn error_policy(_: Arc<GatewayClass>, error: &Error, _: Arc<Self>) -> Action {
        warn!("reconcile failed: {:?}", error);
        Action::requeue(Duration::from_secs(5))
    }

    // TODO: unify with GatewayController in case possible
    async fn patch_status(&self, name: String, status: &GatewayClassStatus) -> Result<()> {
        let gatewayclass_api = Api::<GatewayClass>::all(self.k8s_client.clone());
        let mut conditions = &vec![];
        if let Some(c) = status.conditions.as_ref() {
            conditions = c;
        }
        let patch = Patch::Apply(json!({
            "apiVersion": "gateway.networking.k8s.io/v1",
            "kind": "GatewayClass",
            "status": {
                "conditions": conditions
            }
        }));
        let params = PatchParams::apply(BLIXT_FIELD_MANAGER).force();
        gatewayclass_api
            .patch_status(name.as_str(), &params, &patch)
            .await
            .map_err(K8sError::Client)?;
        Ok(())
    }
}

pub(super) fn check_accepted(gateway_class: &GatewayClass) -> bool {
    let Some(status) = &gateway_class.status else {
        return false;
    };
    let Some(conditions) = &status.conditions else {
        return false;
    };

    conditions
        .iter()
        .any(|c| c.type_ == GatewayConditionType::Accepted.to_string() && c.status == "True")
}

pub(super) fn mark_accepted(gateway_class: &mut GatewayClass) {
    let now = metav1::Time(Utc::now());
    let accepted = metav1::Condition {
        type_: GatewayConditionType::Accepted.to_string(),
        status: String::from("True"),
        reason: GatewayConditionReason::Accepted.to_string(),
        observed_generation: gateway_class.metadata.generation,
        last_transition_time: now,
        message: String::from("Blixt accepts responsibility for this GatewayClass"),
    };
    set_condition(gateway_class, accepted);
}
