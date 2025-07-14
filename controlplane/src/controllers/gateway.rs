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
use gateway_api::apis::standard::gateways::{Gateway, GatewayStatus};
use gateway_api::apis::standard::{
    constants::{GatewayConditionReason, GatewayConditionType},
    gatewayclasses::GatewayClass,
};
use k8s_openapi::api::core::v1::{Service, ServiceSpec, ServiceStatus};
use k8s_openapi::apimachinery::pkg::apis::meta::v1 as metav1;
use kube::{
    Client, Resource, ResourceExt,
    api::{Api, ListParams, Patch, PatchParams},
    runtime::{Controller, controller::Action, watcher::Config},
};
use tracing::{debug, error, info, warn};

use crate::consts::{GATEWAY_CLASS_CONTROLLER_NAME, GATEWAY_SERVICE_LABEL};
use crate::controllers::NamespaceName;
use crate::gateway_utils::{
    create_endpoint_if_not_exists, create_svc_for_gateway, get_accepted_condition,
    get_ingress_ip_len, get_service_key, patch_status, set_gateway_status_addresses,
    set_listener_status, update_service_for_gateway,
};
use crate::route_utils::set_condition;
use crate::{Error, K8sError, Result, gatewayclass_utils};
use thiserror::Error as ThisError;

#[derive(Clone)]
pub struct GatewayController {
    k8s_client: Client,
}

#[derive(ThisError, Debug)]
pub enum GatewayError {
    #[error(transparent)]
    K8s(#[from] K8sError),
    #[error("{0}/{1} does not have any IP address associated")]
    MissingAddresses(String, String),
    #[error("{0}/{1} found {2} IP addresses, currently only a single address is supported")]
    NotExactlyOneIpAddress(String, String, usize),

    #[error("{0}/{1} has an invalid configuration: {2}")]
    InvalidConfiguration(String, String, String),
    #[error("{0}/{1} not ready")]
    NotReady(String, String),
    #[error("{0}/{1} IP not found")]
    IpNotFound(String, String),
    #[error("{0}/{1} addresses of type {2} are not supported; only type IPAddress is supported")]
    AddressTypeNotSupported(String, String, String),
    #[error("{0}/{1} exactly one Service required")]
    NotExactlyOneService(String, String),
    #[error("{0}/{1} does not have any matching Service")]
    MissingService(String, String),
    #[error("{0}/{1} Service does not have a Status")]
    MissingServiceStatus(String, String),
    #[error("{0}/{1} Service does not have an ingress IP assigned")]
    ServiceMissingIngressIp(String, String),
    #[error("{0}/{1} Service does not have an ingress IP assigned")]
    ServiceMissingLoadBalancerIngressIp(String, String),
    #[error("{0}/{1} Service does not have a spec")]
    ServiceMissingLoadBalancerSpec(String, String),
    #[error("{0}/{1} Service does not have a status.loadBalancer.spec")]
    ServiceMissingLoadBalancerStatus(String, String),
    #[error("{0}/{1} Service does not have a status.loadBalancer.ingress")]
    ServiceMissingLoadBalancerIngress(String, String),
    #[error("{0}/{1} Service does not have loadbalancer endpoints in ready state")]
    ServiceMissingLoadBalancerEndpointsReady(String, String),
}

impl GatewayController {
    pub fn new(k8s_client: Client) -> Self {
        Self { k8s_client }
    }

    pub async fn start(self) -> Result<()> {
        let gateway_api = Api::<Gateway>::all(self.k8s_client.clone());
        gateway_api
            .list(&ListParams::default().limit(1))
            .await
            .map_err(K8sError::Client)?; // TODO: map not found

        Controller::new(gateway_api, Config::default().any_semantic())
            .shutdown_on_signal()
            .run(Self::reconcile, Self::error_policy, Arc::new(self))
            .filter_map(|x| async move { std::result::Result::ok(x) })
            .for_each(|_| futures::future::ready(()))
            .await;

        Ok(())
    }

    pub async fn reconcile(gateway: Arc<Gateway>, ctx: Arc<GatewayController>) -> Result<Action> {
        let start = Instant::now();
        let gw_name = gateway.metadata.name()?;
        let namespace = gateway.metadata.namespace()?;

        let gateway_api: Api<Gateway> = Api::namespaced(ctx.k8s_client.clone(), &namespace);
        let mut gw = gateway.as_ref().clone();

        let gateway_class_api = Api::<GatewayClass>::all(ctx.k8s_client.clone());
        let gateway_class = gateway_class_api
            .get(gateway.spec.gateway_class_name.as_str())
            .await
            .map_err(K8sError::Client)?;

        // Only reconcile the Gateway object if it belongs to our controller's gateway class.
        if gateway_class.spec.controller_name.as_str() != GATEWAY_CLASS_CONTROLLER_NAME {
            return Ok(Action::await_change());
        }
        debug!(
            "found a supported GatewayClass: {:?}",
            gateway_class.name_any()
        );

        // Only reconcile the Gateway object if our GatewayClass has already been accepted
        if !gatewayclass_utils::is_accepted(&gateway_class) {
            debug!(
                "GatewayClass {:?} not yet accepted",
                gateway_class.name_any()
            );
            return Ok(Action::await_change());
        }

        set_listener_status(&mut gw)?;
        let accepted_cond = get_accepted_condition(&gw);
        set_condition(&mut gw, accepted_cond.clone());

        // If the controller can't accept responsibility, then set the Condition of type "Programmed" to False and error out.
        if accepted_cond.status == "False" {
            let programmed_cond = metav1::Condition {
                last_transition_time: accepted_cond.last_transition_time.clone(),
                observed_generation: accepted_cond.observed_generation,
                type_: GatewayConditionType::Programmed.to_string(),
                status: "False".to_string(),
                message: accepted_cond.message.clone(),
                reason: GatewayConditionReason::Programmed.to_string(),
            };
            set_condition(&mut gw, programmed_cond);
            patch_status(
                &gateway_api,
                gw_name.clone(),
                gw.status.as_ref().unwrap_or(&GatewayStatus::default()),
            )
            .await?;
            return Err(GatewayError::InvalidConfiguration(
                namespace.clone(),
                gw_name.clone(),
                accepted_cond.message,
            )
            .into());
        }

        // Try to fetch any existing Loadbalancer service(s) for this Gateway.
        let service_api: Api<Service> = Api::namespaced(ctx.k8s_client.clone(), &namespace);
        let services = service_api
            .list(&ListParams::default().labels(&format!("{GATEWAY_SERVICE_LABEL}={gw_name}")))
            .await
            .map_err(K8sError::Client)?;

        if services.items.len() > 1 {
            let mut names: Vec<String> = vec![];
            for svc in services.items {
                if let Some(name) = &svc.meta().name {
                    names.push(name.clone());
                }
            }
            error!(services = ?names, "found multiple Services");
            return Err(GatewayError::NotExactlyOneService(namespace, gw_name).into());
        }

        // If we found a Loadbalancer service, then correct any drift if necessary, else create the service.
        let mut service: Service;
        if let Some(val) = services.items.first() {
            service = val.clone();
            let updated = update_service_for_gateway(gateway.as_ref(), &mut service)?;
            if updated {
                info!("drift detected; updating loadbalancer service");
                let patch_params = PatchParams::default();
                service_api
                    .patch(
                        val.name_any().as_str(),
                        &patch_params,
                        &Patch::Strategic(&service),
                    )
                    .await
                    .map_err(K8sError::Client)?;
            }
        } else {
            info!("creating loadbalancer service");
            service = create_svc_for_gateway(ctx.k8s_client.clone(), gateway.as_ref()).await?;
        }

        // invalid_lb_condition is a Condition that signifies that the Loadbalancer service is invalid.
        let mut invalid_lb_condition = metav1::Condition {
            last_transition_time: metav1::Time(Utc::now()),
            observed_generation: gateway.meta().generation,
            message: "".to_string(),
            reason: GatewayConditionReason::AddressNotAssigned.to_string(),
            status: "False".to_string(),
            type_: GatewayConditionType::Programmed.to_string(),
        };

        let svc_spec: &ServiceSpec = match service.spec.as_ref().ok_or(
            GatewayError::MissingService(namespace.clone(), gw_name.clone()),
        ) {
            Ok(spec) => spec,
            Err(error) => {
                invalid_lb_condition.message = error.to_string();
                set_condition(&mut gw, invalid_lb_condition);
                patch_status(&gateway_api, gw_name, &gw.status.unwrap_or_default()).await?;
                return Err(error.into());
            }
        };

        let svc_status: &ServiceStatus = match service.status.as_ref().ok_or(
            GatewayError::MissingService(namespace.clone(), gw_name.clone()),
        ) {
            Ok(status) => status,
            Err(error) => {
                invalid_lb_condition.message = error.to_string();
                set_condition(&mut gw, invalid_lb_condition);
                patch_status(&gateway_api, gw_name, &gw.status.unwrap_or_default()).await?;
                return Err(error.into());
            }
        };

        let svc_key = get_service_key(&service)?;
        if get_ingress_ip_len(svc_status) == 0 || svc_spec.cluster_ip.is_none() {
            let msg = "LoadBalancer does not have a ingress IP address".to_string();
            invalid_lb_condition.message.clone_from(&msg);
            set_condition(&mut gw, invalid_lb_condition);
            patch_status(
                &gateway_api,
                gw_name.clone(),
                &gw.status.unwrap_or_default(),
            )
            .await?;
            return Err(
                GatewayError::ServiceMissingIngressIp(namespace.clone(), gw_name.clone()).into(),
            );
        }

        create_endpoint_if_not_exists(ctx.k8s_client.clone(), &svc_key, svc_spec, svc_status)
            .await?;
        set_gateway_status_addresses(&mut gw, svc_status);

        let programmed_cond = metav1::Condition {
            last_transition_time: metav1::Time(Utc::now()),
            observed_generation: gateway.meta().generation,
            type_: GatewayConditionType::Programmed.to_string(),
            status: "True".to_string(),
            reason: GatewayConditionReason::Programmed.to_string(),
            message: "Dataplane configured for gateway".to_string(),
        };
        set_condition(&mut gw, programmed_cond);

        patch_status(&gateway_api, gw_name, &gw.status.unwrap_or_default()).await?;

        let duration = Instant::now().sub(start);
        info!("finished reconciling in {:?} ms", duration.as_millis());
        Ok(Action::requeue(Duration::from_secs(60)))
    }

    fn error_policy(_: Arc<Gateway>, error: &Error, _: Arc<GatewayController>) -> Action {
        warn!("reconcile failed: {:?}", error);
        Action::requeue(Duration::from_secs(5))
    }
}
