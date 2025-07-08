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

use crate::{consts::BLIXT_FIELD_MANAGER, *};
use route_utils::set_condition;

use chrono::Utc;
use gateway_api::apis::standard::{
    constants::{GatewayConditionReason, GatewayConditionType},
    gatewayclasses::{GatewayClass, GatewayClassStatus},
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1 as metav1;
use kube::api::{Api, Patch, PatchParams};
use serde_json::json;

pub fn is_accepted(gateway_class: &GatewayClass) -> bool {
    let mut accepted = false;
    if let Some(status) = &gateway_class.status {
        if let Some(conditions) = &status.conditions {
            for condition in conditions {
                accepted = condition.type_ == GatewayConditionType::Accepted.to_string()
                    && condition.status == "True"
            }
        }
    }
    accepted
}

pub fn accept(gateway_class: &mut GatewayClass) {
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

pub async fn patch_status(
    gatewayclass_api: &Api<GatewayClass>,
    name: String,
    status: &GatewayClassStatus,
) -> Result<()> {
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
        .map_err(Error::KubeError)?;
    Ok(())
}
