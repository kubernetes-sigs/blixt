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

use crate::*;
use k8s_gateway_api::{Gateway, GatewayClass, GatewayClassSpec, GatewaySpec};
use k8s_openapi::api::core::v1::Service;
use kube::{
    api::{Api, ListParams, Patch, PatchParams},
    runtime::{controller::Action, watcher::Config, Controller},
    Resource, ResourceExt,
};

use chrono::Utc;
use gateway_utils::*;
use k8s_openapi::apimachinery::pkg::apis::meta::v1 as metav1;
use tracing::*;

pub async fn reconcile(gateway: Arc<Gateway>, ctx: Arc<Context>) -> Result<Action> {
    let start = Instant::now();
    let client = ctx.client.clone();
    let name = gateway.name_any();
    let ns = gateway.namespace().unwrap();

    let gateway_api: Api<Gateway> = Api::namespaced(client.clone(), &ns);
    let gateway_spec: GatewaySpec = gateway.spec.clone();
    let mut gw = Gateway {
        metadata: gateway.metadata.clone(),
        spec: gateway.spec.clone(),
        status: gateway.status.clone(),
    };

    let gateway_class_api = Api::<GatewayClass>::all(client.clone());
    let gateway_class = gateway_class_api
        .get(gateway_spec.gateway_class_name.as_str())
        .await
        .map_err(Error::KubeError)?;

    let gateway_class_spec: GatewayClassSpec = gateway_class.spec.clone();
    if gateway_class_spec.controller_name.as_str() != GATEWAY_CLASS_CONTROLLER_NAME {
        return Ok(Action::requeue(Duration::from_secs(3600)));
    }
    debug!(
        "found a supported GatewayClass: {:?}",
        gateway_class.name_any()
    );

    set_listener_status(&mut gw);
    let accepted_cond = get_accepted_condition(&mut gw);
    set_condition(&mut gw, accepted_cond.clone());
    if accepted_cond.status == "False".to_string() {
        let programmed_cond = metav1::Condition {
            last_transition_time: accepted_cond.last_transition_time.clone(),
            observed_generation: accepted_cond.observed_generation,
            type_: "Programmed".to_string(),
            status: "False".to_string(),
            message: accepted_cond.message.clone(),
            reason: "Invalid".to_string(),
        };
        set_condition(&mut gw, programmed_cond);
    }

    if accepted_cond.status == String::from("False") {
        patch_status(&gateway_api, name, gw.status.as_ref().unwrap()).await?;
        return Err(Error::InvalidConfigError(accepted_cond.message));
    }

    let service_api: Api<Service> = Api::namespaced(client, &ns);
    let services = service_api
        .list(&ListParams::default().labels(&format!("{}={}", GATEWAY_SERVICE_LAEBL, name)))
        .await
        .map_err(Error::KubeError)?;
    if services.items.len() > 1 {
        return Err(Error::LoadBalancerError(
            "found more than 1 Service for this Gateway; multiple services are not supported"
                .to_string(),
        ));
    }

    let mut service: Service;
    if let Some(val) = services.items.get(0) {
        service = val.clone();
        let updated = update_service_for_gateway(gateway.as_ref(), &mut service)?;
        if updated {
            info!("drift detected; updating loadbalancer service");
            let patch_parmas = PatchParams::default();
            service_api
                .patch(
                    val.name_any().as_str(),
                    &patch_parmas,
                    &Patch::Strategic(&service),
                )
                .await
                .map_err(Error::KubeError)?;
        }
    } else {
        info!("creating loadbalancer service");
        service = create_svc_for_gateway(ctx.clone(), gateway.as_ref()).await?;
    }

    let now = k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(Utc::now());
    if get_ingress_ip_len(&service) == 0 || service.spec.as_ref().unwrap().cluster_ip.is_none() {
        let msg = "LoadBalancer does not have a ingress IP address".to_string();
        set_condition(
            &mut gw,
            metav1::Condition {
                last_transition_time: now,
                observed_generation: gateway.meta().generation,
                message: msg.clone(),
                reason: "AddressNotAssigned".to_string(),
                status: "False".to_string(),
                type_: "Programmed".to_string(),
            },
        );
        patch_status(&gateway_api, name, &gw.status.unwrap()).await?;
        return Err(Error::LoadBalancerError(msg));
    }

    create_endpoint_if_not_exists(ctx.clone(), &service).await?;
    set_gateway_status_addresses(&mut gw, &service);

    let programmed_cond = metav1::Condition {
        last_transition_time: now,
        observed_generation: gateway.meta().generation,
        type_: "Programmed".to_string(),
        status: "True".to_string(),
        reason: "Programmed".to_string(),
        message: "Dataplane configured for gateway".to_string(),
    };
    set_condition(&mut gw, programmed_cond);

    patch_status(&gateway_api, name, gw.status.as_ref().unwrap()).await?;

    let duration = Instant::now().sub(start);
    info!("finished reconciling in {:?} ms", duration.as_millis());
    Ok(Action::requeue(Duration::from_secs(60)))
}

pub async fn controller(ctx: Context) -> Result<()> {
    let gateway = Api::<Gateway>::all(ctx.client.clone());
    gateway
        .list(&ListParams::default().limit(1))
        .await
        .map_err(Error::CRDNotFoundError)?;

    Controller::new(gateway, Config::default().any_semantic())
        .shutdown_on_signal()
        .run(reconcile, error_policy, Arc::new(ctx))
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()))
        .await;

    Ok(())
}

fn error_policy(_: Arc<Gateway>, error: &Error, _: Arc<Context>) -> Action {
    warn!("reconcile failed: {:?}", error);
    Action::requeue(Duration::from_secs(5))
}
