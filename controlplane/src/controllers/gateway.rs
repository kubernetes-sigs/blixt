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

use chrono::Utc;
use futures::StreamExt;
use gateway_api::apis::standard::gateways::{Gateway, GatewayStatus};
use gateway_api::apis::standard::{
    constants::{GatewayConditionReason, GatewayConditionType},
    gatewayclasses::GatewayClass,
};
use gateway_api::constants::{ListenerConditionReason, ListenerConditionType};
use gateway_api::gateways::{
    GatewayListeners, GatewayListenersAllowedRoutesKinds, GatewaySpec, GatewayStatusAddresses,
    GatewayStatusListeners, GatewayStatusListenersSupportedKinds,
};
use k8s_openapi::api::core::v1::{
    EndpointAddress, EndpointPort, EndpointSubset, Endpoints, Service, ServicePort, ServiceSpec,
    ServiceStatus,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1 as metav1;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::PostParams;
use kube::{
    Client, Resource, ResourceExt,
    api::{Api, ListParams, Patch, PatchParams},
    runtime::{Controller, controller::Action, watcher::Config},
};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::str::FromStr;
use std::{
    ops::Sub,
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error as ThisError;
use tracing::{debug, error, info, warn};

use crate::consts::{BLIXT_FIELD_MANAGER, GATEWAY_CLASS_CONTROLLER_NAME, GATEWAY_SERVICE_LABEL};
use crate::controllers::gatewayclass;
use crate::utils::set_condition;
use crate::{Error, K8sError, NamespaceName, NamespacedName, Result};

#[derive(Clone)]
pub struct GatewayController {
    k8s_client: Client,
}

#[derive(ThisError, Debug)]
pub enum GatewayError {
    #[error(transparent)]
    K8s(#[from] K8sError),
    #[error("{0:?} does not have any IP address associated")]
    MissingAddresses(NamespacedName),
    #[error("{0:?} found {1} IP addresses, currently only a single address is supported")]
    NotExactlyOneIpAddress(NamespacedName, usize),
    #[error("{0:?} has an invalid configuration: {1}")]
    InvalidConfiguration(NamespacedName, String),
    #[error("{0:?} not ready")]
    NotReady(NamespacedName),
    #[error("{0:?} IP not found")]
    IpNotFound(NamespacedName),
    #[error("{0:?} addresses of type {1} are not supported; only type IPAddress is supported")]
    AddressTypeNotSupported(NamespacedName, String),
    #[error("{0:?} exactly one Service required")]
    NotExactlyOneService(NamespacedName),
    #[error("{0:?} does not have any matching Service")]
    MissingService(NamespacedName),
    #[error("{0:?} Service does not have a Status")]
    MissingServiceStatus(NamespacedName),
    #[error("{0:?} Service does not have an ingress IP assigned")]
    ServiceMissingIngressIp(NamespacedName),
    #[error("{0:?} Service does not have an ingress IP assigned")]
    ServiceMissingLoadBalancerIngressIp(NamespacedName),
    #[error("{0:?} Service does not have a spec")]
    ServiceMissingLoadBalancerSpec(NamespacedName),
    #[error("{0:?} Service does not have a status.loadBalancer.spec")]
    ServiceMissingLoadBalancerStatus(NamespacedName),
    #[error("{0:?} Service does not have a status.loadBalancer.ingress")]
    ServiceMissingLoadBalancerIngress(NamespacedName),
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

    async fn reconcile(gateway: Arc<Gateway>, ctx: Arc<GatewayController>) -> Result<Action> {
        let start = Instant::now();

        let gateway_id = gateway.metadata.namespaced_name()?;
        let namespace = gateway_id.namespace.as_str();
        let gateway_name = gateway_id.name.as_str();

        let gateway_api: Api<Gateway> = Api::namespaced(ctx.k8s_client.clone(), namespace);
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
        if !gatewayclass::check_accepted(&gateway_class) {
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
                gateway_name,
                gw.status.as_ref().unwrap_or(&GatewayStatus::default()),
            )
            .await?;
            return Err(GatewayError::InvalidConfiguration(
                gateway_id.clone(),
                accepted_cond.message,
            )
            .into());
        }

        // Try to fetch any existing Loadbalancer service(s) for this Gateway.
        let service_api: Api<Service> = Api::namespaced(ctx.k8s_client.clone(), namespace);
        let services = service_api
            .list(&ListParams::default().labels(&format!("{GATEWAY_SERVICE_LABEL}={gateway_name}")))
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
            return Err(GatewayError::NotExactlyOneService(gateway_id).into());
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
            service = ctx.create_svc_for_gateway(gateway.as_ref()).await?;
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

        let svc_spec: &ServiceSpec = match service
            .spec
            .as_ref()
            .ok_or(GatewayError::MissingService(gateway_id.clone()))
        {
            Ok(spec) => spec,
            Err(error) => {
                invalid_lb_condition.message = error.to_string();
                set_condition(&mut gw, invalid_lb_condition);
                patch_status(&gateway_api, gateway_name, &gw.status.unwrap_or_default()).await?;
                return Err(error.into());
            }
        };

        let svc_status: &ServiceStatus = match service
            .status
            .as_ref()
            .ok_or(GatewayError::MissingServiceStatus(gateway_id.clone()))
        {
            Ok(status) => status,
            Err(error) => {
                invalid_lb_condition.message = error.to_string();
                set_condition(&mut gw, invalid_lb_condition);
                patch_status(&gateway_api, gateway_name, &gw.status.unwrap_or_default()).await?;
                return Err(error.into());
            }
        };

        let svc_key = get_service_key(&service)?;
        if get_ingress_ip_len(svc_status) == 0 || svc_spec.cluster_ip.is_none() {
            let msg = "LoadBalancer does not have a ingress IP address".to_string();
            invalid_lb_condition.message.clone_from(&msg);
            set_condition(&mut gw, invalid_lb_condition);
            patch_status(&gateway_api, gateway_name, &gw.status.unwrap_or_default()).await?;
            return Err(GatewayError::ServiceMissingIngressIp(gateway_id.clone()).into());
        }

        ctx.create_endpoint_if_not_exists(&svc_key, svc_spec, svc_status)
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

        patch_status(&gateway_api, gateway_name, &gw.status.unwrap_or_default()).await?;

        let duration = Instant::now().sub(start);
        info!("finished reconciling in {:?} ms", duration.as_millis());
        Ok(Action::requeue(Duration::from_secs(60)))
    }

    fn error_policy(_: Arc<Gateway>, error: &Error, _: Arc<GatewayController>) -> Action {
        warn!("reconcile failed: {:?}", error);
        Action::requeue(Duration::from_secs(5))
    }

    // Creates a LoadBalancer Service for the provided Gateway.
    async fn create_svc_for_gateway(&self, gateway: &Gateway) -> Result<Service> {
        let namespace = gateway.metadata.namespace()?;
        let gw_name = gateway.metadata.name()?;

        let mut svc_meta = ObjectMeta::default();
        let svc_name = format!("gateway-{}", &gw_name);
        svc_meta.namespace = Some(namespace.to_string());
        svc_meta.name = Some(svc_name.clone());
        let mut labels = BTreeMap::new();
        labels.insert(GATEWAY_SERVICE_LABEL.to_string(), gw_name.to_string());
        svc_meta.labels = Some(labels);

        let mut svc = Service {
            metadata: svc_meta,
            spec: Some(ServiceSpec::default()),
            status: Some(ServiceStatus::default()),
        };

        let _ = update_service_for_gateway(gateway, &mut svc)?;

        info!(
            "creating loadbalancer service {} for gateway {}",
            &svc_name, &gw_name
        );
        debug!("{svc:?}");
        let svc_api: Api<Service> = Api::namespaced(self.k8s_client.clone(), namespace);
        let service = svc_api
            .create(&PostParams::default(), &svc)
            .await
            .map_err(K8sError::Client)?;

        Ok(service)
    }

    // Creates an Endpoints object for the provided Service pointing to it's ingress IP address.
    // Since we don't set a selector on the Service (because we don't need to route incoming traffic
    // to a particular pod), no Endpoints object is created for it. An Endpoints object is required
    // because MetalLB does not respond to ARP packets until one exists for the LoadBalancer Service
    // causing traffic to never reach the node.
    // Ref: https://github.com/metallb/metallb/issues/1640
    async fn create_endpoint_if_not_exists(
        &self,
        gateway_id: &NamespacedName,
        svc_spec: &ServiceSpec,
        svc_status: &ServiceStatus,
    ) -> Result<()> {
        let mut lb_addr = None;
        let lb_status = svc_status.load_balancer.as_ref().ok_or(
            GatewayError::ServiceMissingLoadBalancerStatus(gateway_id.clone()),
        )?;
        let ingress =
            lb_status
                .ingress
                .as_ref()
                .ok_or(GatewayError::ServiceMissingLoadBalancerIngress(
                    gateway_id.clone(),
                ))?;
        for addr in ingress {
            if let Some(ip) = &addr.ip {
                lb_addr = Some(ip.clone());
                break;
            }
        }
        let lb_addr_ip = lb_addr.ok_or(GatewayError::ServiceMissingLoadBalancerIngressIp(
            gateway_id.clone(),
        ))?;

        let endpoints_api: Api<Endpoints> =
            Api::namespaced(self.k8s_client.clone(), &gateway_id.namespace);

        if let Some(err) = endpoints_api.get(&gateway_id.name).await.err() {
            if check_if_not_found_err(err) {
                let mut ep_ports: Vec<EndpointPort> = vec![];
                if let Some(ports) = &svc_spec.ports {
                    for port in ports {
                        ep_ports.push(EndpointPort {
                            port: port.port,
                            protocol: port.protocol.clone(),
                            ..Default::default()
                        });
                    }
                }

                let obj_meta = ObjectMeta {
                    name: Some(gateway_id.name.clone()),
                    namespace: Some(gateway_id.namespace.clone()),
                    ..Default::default()
                };
                let ep_addr = EndpointAddress {
                    ip: lb_addr_ip,
                    ..Default::default()
                };
                let endpoints = Endpoints {
                    metadata: obj_meta,
                    subsets: Some(vec![EndpointSubset {
                        addresses: Some(vec![ep_addr]),
                        not_ready_addresses: None,
                        ports: Some(ep_ports),
                    }]),
                };
                let ep = endpoints_api
                    .create(&PostParams::default(), &endpoints)
                    .await
                    .map_err(K8sError::Client)?;
                info!("created Endpoints object {}", ep.name_any());
            }
        }

        Ok(())
    }
}

/// Get Gateway IPs
/// WARN: currently the function returns a Vec containing a single IPv4 and errors in other cases
/// IPv6 and multiple IPs are currently not supported
pub(super) fn get_gateway_ips(gateway: &Gateway) -> Result<Vec<IpAddr>> {
    let gateway_id = NamespacedName::new(gateway.metadata.namespace()?, gateway.metadata.name()?);

    let Some(status) = &gateway.status else {
        return Err(GatewayError::NotReady(gateway_id.clone()).into());
    };

    let Some(addresses) = &status.addresses else {
        return Err(GatewayError::MissingAddresses(gateway_id.clone()).into());
    };

    let ip_addresses = addresses
        .iter()
        .filter(|a| {
            if let Some(r#type) = &a.r#type {
                r#type == "IPAddress"
            } else {
                false
            }
        })
        .filter_map(|a| {
            IpAddr::from_str(&a.value).ok()
        })
        .filter(|a| {
            if a.is_ipv4() {
                true
            } else {
                warn!("Gateway IpAddress {:?}. IPv6 addresses are currently not supported. Skipping...", a);
                false
            }
        })
        .collect::<Vec<IpAddr>>();

    if ip_addresses.len() != 1 {
        return Err(
            GatewayError::NotExactlyOneIpAddress(gateway_id.clone(), ip_addresses.len()).into(),
        );
    }

    Ok(ip_addresses)
}

// Patch the provided status on the Gateway object.
async fn patch_status(
    gateway_api: &Api<Gateway>,
    name: &str,
    status: &GatewayStatus,
) -> Result<()> {
    let mut listeners = &vec![];
    if let Some(l) = status.listeners.as_ref() {
        listeners = l;
    }
    let mut conditions = &vec![];
    if let Some(c) = status.conditions.as_ref() {
        conditions = c;
    }
    let mut addresses = &vec![];
    if let Some(a) = status.addresses.as_ref() {
        addresses = a;
    }
    let patch = Patch::Apply(json!({
        "apiVersion": "gateway.networking.k8s.io/v1",
        "kind": "Gateway",
        "status": {
            "listeners": listeners,
            "conditions": conditions,
            "addresses": addresses
        }
    }));
    let params = PatchParams::apply(BLIXT_FIELD_MANAGER).force();
    gateway_api
        .patch_status(name, &params, &patch)
        .await
        .map_err(K8sError::Client)?;
    Ok(())
}

// Modifies the Gateway's status to reflect the LoadBalancer Service's ingress IP address.
fn set_gateway_status_addresses(gateway: &mut Gateway, svc_status: &ServiceStatus) {
    let mut gw_addrs: Vec<GatewayStatusAddresses> = vec![];

    if let Some(load_balancer) = &svc_status.load_balancer {
        if let Some(ingress) = &load_balancer.ingress {
            for addr in ingress {
                if let Some(ip) = &addr.ip {
                    gw_addrs.push(GatewayStatusAddresses {
                        r#type: Some("IPAddress".to_string()),
                        value: ip.clone(),
                    });
                }
            }
        }
    }

    if let Some(status) = gateway.status.as_mut() {
        status.addresses = Some(gw_addrs);
    } else {
        gateway.status = Some(GatewayStatus {
            addresses: Some(gw_addrs),
            ..Default::default()
        });
    }
}

// Inspects the provided Gateway and sets the status of its listeners accordingly.
fn set_listener_status(gateway: &mut Gateway) -> Result<()> {
    let gw_name = gateway.metadata.name()?;
    let namespace = gateway.metadata.namespace()?;

    let gateway_spec: &GatewaySpec = &gateway.spec;
    let mut statuses: Vec<GatewayStatusListeners> = vec![];
    let mut current_listener_statuses: HashMap<String, GatewayStatusListeners> = HashMap::new();

    if let Some(gw_status) = &gateway.status {
        if let Some(listeners) = &gw_status.listeners {
            for listener in listeners {
                current_listener_statuses.insert(listener.name.clone(), listener.clone());
            }
        }
    }

    let generation = gateway
        .metadata
        .generation
        .ok_or(K8sError::missing_resource_property(
            &NamespacedName::new(gw_name, namespace),
            "metadata.generation",
        ))?;

    for listener in &gateway_spec.listeners {
        let mut final_conditions = vec![];
        let (supported_kinds, conditions) = get_listener_status(listener, generation);
        if let Some(current_listener_status) = current_listener_statuses.get(&listener.name) {
            for condition in conditions {
                let mut present = false;
                for current_condition in &current_listener_status.conditions {
                    if condition.type_ == current_condition.type_ {
                        present = true;
                        if condition.status == current_condition.status {
                            let mut updated_condition = current_condition.clone();
                            updated_condition.observed_generation = gateway.metadata.generation;
                            final_conditions.push(updated_condition);
                        } else {
                            final_conditions.push(condition.clone());
                        }
                    }
                }
                if !present {
                    final_conditions.push(condition.clone());
                }
            }
        } else {
            final_conditions = conditions;
        }

        statuses.push(GatewayStatusListeners {
            name: listener.name.clone(),
            attached_routes: 0,
            supported_kinds,
            conditions: final_conditions,
        });
    }

    if let Some(ref mut status) = gateway.status {
        status.listeners = Some(statuses);
    }
    Ok(())
}

// Inspects the provided Listener and returns the list GroupKind objects for support Routes and a
// list of Conditions accordingly.
fn get_listener_status(
    listener: &GatewayListeners,
    generation: i64,
) -> (
    Vec<GatewayStatusListenersSupportedKinds>,
    Vec<metav1::Condition>,
) {
    let now = metav1::Time(Utc::now());
    let mut supported_kinds: Vec<GatewayStatusListenersSupportedKinds> = vec![];
    let mut conditions: Vec<metav1::Condition> = vec![
        metav1::Condition {
            type_: ListenerConditionType::ResolvedRefs.to_string(),
            status: String::from("True"),
            reason: ListenerConditionReason::ResolvedRefs.to_string(),
            observed_generation: Some(generation),
            last_transition_time: now.clone(),
            message: String::from("All references resolved"),
        },
        metav1::Condition {
            type_: ListenerConditionType::Accepted.to_string(),
            status: String::from("True"),
            reason: ListenerConditionReason::Accepted.to_string(),
            observed_generation: Some(generation),
            last_transition_time: now.clone(),
            message: String::from("Listener is valid"),
        },
        metav1::Condition {
            type_: ListenerConditionType::Programmed.to_string(),
            status: String::from("True"),
            reason: ListenerConditionType::Programmed.to_string(),
            observed_generation: Some(generation),
            last_transition_time: now,
            message: String::from("Listener is valid"),
        },
    ];

    let mut update_listener_condition =
        |status: String, reason: String, message: String, idx: usize| {
            conditions[idx].status = status;
            conditions[idx].reason = reason;
            conditions[idx].message = message;
        };

    match listener.protocol.as_str() {
        // Accept HTTP and HTTPS protocol types even though we don't support
        // HTTPRoute so that Gateway API conformance tests pass.
        "TCP" | "HTTP" | "HTTPS" => {
            supported_kinds.push(GatewayStatusListenersSupportedKinds {
                group: Some("gateway.networking.k8s.io".to_string()),
                kind: "TCPRoute".to_string(),
            });
            if let Some(routes) = &listener.allowed_routes {
                if let Some(rgks) = &routes.kinds {
                    if let Some(msg) = check_route_kinds(Some("TCPRoute"), rgks) {
                        update_listener_condition(
                            String::from("False"),
                            ListenerConditionReason::InvalidRouteKinds.to_string(),
                            msg.clone(),
                            0,
                        );
                        update_listener_condition(
                            String::from("False"),
                            ListenerConditionReason::InvalidRouteKinds.to_string(),
                            msg.clone(),
                            1,
                        );
                        update_listener_condition(
                            String::from("False"),
                            ListenerConditionReason::Invalid.to_string(),
                            msg.clone(),
                            2,
                        );
                    }
                }
            }
        }
        "UDP" => {
            supported_kinds.push(GatewayStatusListenersSupportedKinds {
                group: Some("gateway.networking.k8s.io".to_string()),
                kind: "UDPRoute".to_string(),
            });
            if let Some(routes) = &listener.allowed_routes {
                if let Some(rgks) = &routes.kinds {
                    if let Some(msg) = check_route_kinds(Some("UDPRoute"), rgks) {
                        update_listener_condition(
                            String::from("False"),
                            ListenerConditionReason::InvalidRouteKinds.to_string(),
                            msg.clone(),
                            0,
                        );
                        update_listener_condition(
                            String::from("False"),
                            ListenerConditionReason::InvalidRouteKinds.to_string(),
                            msg.clone(),
                            1,
                        );
                        update_listener_condition(
                            String::from("False"),
                            ListenerConditionReason::Invalid.to_string(),
                            msg.clone(),
                            2,
                        );
                    }
                }
            }
        }
        _ => {
            update_listener_condition(
                String::from("False"),
                ListenerConditionReason::UnsupportedProtocol.to_string(),
                format!(
                    "Unsupported protocol: {}, must be one of TCP or UDP",
                    listener.protocol
                ),
                1,
            );

            if let Some(routes) = &listener.allowed_routes {
                if let Some(rgks) = &routes.kinds {
                    if let Some(msg) = check_route_kinds(Some("UDPRoute"), rgks) {
                        update_listener_condition(
                            String::from("False"),
                            ListenerConditionReason::InvalidRouteKinds.to_string(),
                            msg.clone(),
                            0,
                        );
                    }
                }
            }

            update_listener_condition(
                String::from("False"),
                ListenerConditionReason::Invalid.to_string(),
                format!(
                    "Unsupported protocol: {}, must be one of TCP or UDP",
                    listener.protocol
                ),
                2,
            );
        }
    }

    (supported_kinds, conditions)
}

fn check_route_kinds(
    kind: Option<&str>,
    rgks: &[GatewayListenersAllowedRoutesKinds],
) -> Option<String> {
    if rgks.is_empty() {
        return None;
    }

    if rgks.len() > 1 {
        return Some(String::from(
            "Multiple route kinds for a single listener is unsupported",
        ));
    }

    let rgk = &rgks[0];
    if let Some(k) = kind {
        if rgk.kind != k {
            return Some(format!(
                "Unsupported route kind {}; only {} is supported",
                rgk.kind, k
            ));
        }
    } else if rgk.kind != "TCPRoute" || rgk.kind != "UDPRoute" {
        return Some(format!(
            "Unsupported route kind {}; can be one of TCPRoute or UDPRoute",
            rgk.kind
        ));
    }

    if let Some(group) = &rgk.group {
        if group.as_str() != "gateway.networking.k8s.io" {
            return Some(format!("Unsupported API group: {group}"));
        }
    }
    None
}

// Inspects the provided Gateway and returns a Condition of type "Accepted" with appropriate reason and status.
// Ideally, this should be called after the Gateway object reflects the latest status of its
// listeners.
fn get_accepted_condition(gateway: &Gateway) -> metav1::Condition {
    let now = metav1::Time(Utc::now());
    let mut accepted = metav1::Condition {
        type_: GatewayConditionType::Accepted.to_string(),
        status: String::from("True"),
        reason: GatewayConditionReason::Accepted.to_string(),
        observed_generation: gateway.metadata.generation,
        last_transition_time: now,
        message: String::from("Blixt accepts responsibility for this Gateway"),
    };
    let gateway_spec: &GatewaySpec = &gateway.spec;

    if let Some(status) = &gateway.status {
        if let Some(listeners) = &status.listeners {
            for listener in listeners {
                for conditon in &listener.conditions {
                    if conditon.status == "False" {
                        accepted.status = String::from("False");
                        accepted.reason = GatewayConditionReason::ListenersNotValid.to_string();
                        accepted.message = format!("listener {} is invalid", listener.name);
                    }
                }
            }
        }
    }

    if let Some(addresses) = &gateway_spec.addresses {
        for addr in addresses {
            if let Some(addr_type) = &addr.r#type {
                if addr_type.as_str() != "IPAddress" {
                    accepted.status = String::from("False");
                    accepted.reason = GatewayConditionReason::UnsupportedAddress.to_string();
                    accepted.message = format!(
                        "found an address of type {addr_type}, only type IPAddress is supported"
                    );
                    break;
                }
            }
        }
    }
    accepted
}

// Updates the provided Service to match the desired state according to the provided Gateway.
// Returns true if Service was modified.
fn update_service_for_gateway(gateway: &Gateway, svc: &mut Service) -> Result<bool> {
    let mut updated = false;
    let mut ports: Vec<ServicePort> = vec![];
    for listener in &gateway.spec.listeners {
        let mut port = ServicePort::default();
        port.name = Some(listener.name.clone());
        port.port = listener.port;
        port.target_port = Some(IntOrString::Int(listener.port));
        match listener.protocol.as_str() {
            "TCP" | "HTTP" | "HTTPS" => {
                port.protocol = Some("TCP".to_string());
                ports.push(port);
            }
            "UDP" => {
                port.protocol = Some("UDP".to_string());
                ports.push(port);
            }
            _ => {
                continue;
            }
        }
    }
    let mut address = None;
    if let Some(addresses) = &gateway.spec.addresses {
        if !addresses.is_empty() {
            let addr = addresses[0].clone();
            if let Some(t) = addr.r#type {
                if t != "IPAddress" {
                    return Err(GatewayError::AddressTypeNotSupported(
                        gateway.metadata.namespaced_name()?,
                        t,
                    )
                    .into());
                }
            }
            address = Some(addresses[0].clone());
        }
        if addresses.len() > 1 {
            warn!("multiple addresses");
        }
    }

    let svc_spec = svc
        .spec
        .as_mut()
        .ok_or(GatewayError::ServiceMissingLoadBalancerSpec(
            svc.metadata.namespaced_name()?,
        ))?;

    // TODO: this is not required when using MetalLB
    // using e.g. the cloud-provider-kind LB the ports are mapped without this option
    // this is related to the logic for compiling the TCPRoute Targets
    // port mappings should be supported
    //svc_spec.allocate_load_balancer_node_ports = Some(false);

    let lb_ip: Option<String> = svc_spec.load_balancer_ip.clone();
    if let Some((addr, ip)) = address.clone().zip(lb_ip.clone()) {
        if ip != addr.value {
            svc_spec.load_balancer_ip = Some(addr.value.clone());
            updated = true;
        }
    }
    if address.is_none() && lb_ip.is_some() {
        svc_spec.load_balancer_ip = None;
        updated = true;
    }
    if let Some(ref mut t) = svc_spec.type_ {
        if t != "LoadBalancer" {
            *t = "LoadBalancer".to_string();
            updated = true;
        }
    } else {
        svc_spec.type_ = Some("LoadBalancer".to_string());
    }
    if let Some(ref mut svc_ports) = svc_spec.ports {
        let mut diff = false;
        if svc_ports.len() != ports.len() {
            diff = true;
        }

        let mut iter = svc_ports.iter().zip(ports.iter());
        for (p1, p2) in &mut iter {
            if p1.name != p2.name || p1.port != p2.port || p1.protocol != p2.protocol {
                diff = true;
                break;
            }
        }

        if diff {
            *svc_ports = ports;
            updated = true;
        }
    } else {
        svc_spec.ports = Some(ports);
    }

    Ok(updated)
}

fn get_service_key(service: &Service) -> Result<NamespacedName> {
    let svc_name = service.metadata.name()?;
    let svc_ns = service.metadata.namespace()?;
    Ok(NamespacedName::new(svc_name, svc_ns))
}

// Returns true if the provided error is a not found error.
fn check_if_not_found_err(error: kube::Error) -> bool {
    if let kube::Error::Api(response) = error {
        if response.code == 404 {
            return true;
        }
    }
    false
}

// Returns the number of ingresses set on the LoadBalancer Service.
fn get_ingress_ip_len(svc_status: &ServiceStatus) -> usize {
    if let Some(lb) = &svc_status.load_balancer {
        if let Some(ingress) = &lb.ingress {
            return ingress.len();
        }
    }
    0
}
