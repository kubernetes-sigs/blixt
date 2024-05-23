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

#![allow(clippy::field_reassign_with_default)]

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use crate::*;
use gateway_api::apis::standard::{
    constants::{
        GatewayConditionReason, GatewayConditionType, ListenerConditionReason,
        ListenerConditionType,
    },
    gateways::{
        Gateway, GatewayListeners, GatewayListenersAllowedRoutesKinds, GatewaySpec, GatewayStatus,
        GatewayStatusAddresses, GatewayStatusListeners, GatewayStatusListenersSupportedKinds,
    },
};
use kube::{
    api::{Api, Patch, PatchParams, PostParams},
    core::ObjectMeta,
    Resource, ResourceExt,
};

use k8s_openapi::api::core::v1::{
    EndpointAddress, EndpointPort, EndpointSubset, Endpoints, Service, ServicePort, ServiceSpec,
    ServiceStatus,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1 as metav1;

use chrono::Utc;
use serde_json::json;
use tracing::*;

// Modifies the Gateway's status to reflect the LoadBalancer Service's ingress IP address.
pub fn set_gateway_status_addresses(gateway: &mut Gateway, svc_status: &ServiceStatus) {
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
        let mut status = GatewayStatus::default();
        status.addresses = Some(gw_addrs);
        gateway.status = Some(status);
    }
}

// Creates an Endpoints object for the provided Service pointing to it's ingress IP address.
// Since we don't set a selector on the Service (because we don't need to route incoming traffic
// to a particular pod), no Endpoints object is created for it. An Endpoints object is required
// because MetalLB does not respond to ARP packets until one exists for the LoadBalancer Service
// causing traffic to never reach the node.
// Ref: https://github.com/metallb/metallb/issues/1640
pub async fn create_endpoint_if_not_exists(
    ctx: Arc<Context>,
    key: &NamespacedName,
    svc_spec: &ServiceSpec,
    svc_status: &ServiceStatus,
) -> Result<()> {
    let mut lb_addr = None;
    let lb_status = svc_status
        .load_balancer
        .as_ref()
        .ok_or(Error::LoadBalancerError(
            "Load balancer not found in service status".to_string(),
        ))?;
    let ingress = lb_status.ingress.as_ref().ok_or(Error::LoadBalancerError(
        "Ingress not found in service status".to_string(),
    ))?;
    for addr in ingress {
        if let Some(ip) = &addr.ip {
            lb_addr = Some(ip.clone());
            break;
        }
    }
    let lb_addr_ip = lb_addr.ok_or(Error::LoadBalancerError(
        "LoadBalancer ingress ip not found in service status".to_string(),
    ))?;

    let endpoints_api: Api<Endpoints> = Api::namespaced(ctx.client.clone(), &key.namespace);

    if let Some(err) = endpoints_api.get(&key.name).await.err() {
        if check_if_not_found_err(err) {
            let mut ep_ports: Vec<EndpointPort> = vec![];
            if let Some(ports) = &svc_spec.ports {
                for port in ports {
                    let mut ep_port = EndpointPort::default();
                    ep_port.port = port.port;
                    ep_port.protocol.clone_from(&port.protocol);
                    ep_ports.push(ep_port);
                }
            }

            let mut obj_meta = ObjectMeta::default();
            obj_meta.name = Some(key.name.clone());
            obj_meta.namespace = Some(key.namespace.clone());

            let mut ep_addr = EndpointAddress::default();
            ep_addr.ip = lb_addr_ip;

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
                .map_err(Error::KubeError)?;
            info!("created Endpoints object {}", ep.name_any());
        }
    }

    Ok(())
}

// Returns true if the provided error is a not found error.
pub fn check_if_not_found_err(error: kube::Error) -> bool {
    if let kube::Error::Api(response) = error {
        if response.code == 404 {
            return true;
        }
    }
    false
}

// Returns the number of ingresses set on the LoadBalancer Service.
pub fn get_ingress_ip_len(svc_status: &ServiceStatus) -> usize {
    if let Some(lb) = &svc_status.load_balancer {
        if let Some(ingress) = &lb.ingress {
            return ingress.len();
        }
    }
    0
}

// Creates a LoadBalancer Service for the provided Gateway.
pub async fn create_svc_for_gateway(ctx: Arc<Context>, gateway: &Gateway) -> Result<Service> {
    let mut svc_meta = ObjectMeta::default();
    let ns = gateway.namespace().unwrap_or("default".to_string());
    svc_meta.namespace = Some(ns.clone());
    svc_meta.generate_name = Some(format!("service-for-gateway-{}-", gateway.name_any()));

    let mut labels = BTreeMap::new();
    labels.insert(GATEWAY_SERVICE_LABEL.to_string(), gateway.name_any());
    svc_meta.labels = Some(labels);

    let mut svc = Service {
        metadata: svc_meta,
        spec: Some(ServiceSpec::default()),
        status: Some(ServiceStatus::default()),
    };
    update_service_for_gateway(gateway, &mut svc)?;

    let svc_api: Api<Service> = Api::namespaced(ctx.client.clone(), ns.as_str());
    let service = svc_api
        .create(&PostParams::default(), &svc)
        .await
        .map_err(Error::KubeError)?;

    Ok(service)
}

// Updates the provided Service to match the desired state according to the provided Gateway.
// Returns true if Service was modified.
pub fn update_service_for_gateway(gateway: &Gateway, svc: &mut Service) -> Result<bool> {
    let mut updated = false;
    let mut ports: Vec<ServicePort> = vec![];
    for listener in &gateway.spec.listeners {
        let mut port = ServicePort::default();
        port.name = Some(listener.name.clone());
        port.port = listener.port;
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
                    return Err(Error::InvalidConfigError(format!("addresses of type {} are not supported; only type IPAddress is supported", t).to_string()));
                }
            }
            address = Some(addresses[0].clone());
        }
        if addresses.len() > 1 {
            warn!("multiple addresses");
        }
    }
    let svc_spec = svc.spec.as_mut().ok_or(Error::LoadBalancerError(
        "Loadbalancer service does not have a spec".to_string(),
    ))?;

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

// Patch the provided status on the Gateway object.
pub async fn patch_status(
    gateway_api: &Api<Gateway>,
    name: String,
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
        .patch_status(name.as_str(), &params, &patch)
        .await
        .map_err(Error::KubeError)?;
    Ok(())
}

// Sets the provided condition on the Gateway object. The condition on the Gateway is only updated
// if the new condition has a different status (except for the observed generation which is always
// updated).
pub fn set_condition(gateway: &mut Gateway, new_cond: metav1::Condition) {
    if let Some(ref mut status) = gateway.status {
        if let Some(ref mut conditions) = status.conditions {
            for condition in conditions.iter_mut() {
                if condition.type_ == new_cond.type_ {
                    if condition.status == new_cond.status {
                        // always update the observed generation
                        condition.observed_generation = new_cond.observed_generation;
                        return;
                    }
                    *condition = new_cond;
                    return;
                }
            }
            conditions.push(new_cond);
        } else {
            status.conditions = Some(vec![new_cond]);
        }
    }
}

// Inspects the provided Gateway and returns a Condition of type "Accepted" with appropriate reason and status.
// Ideally, this should be called after the Gateway object reflects the latest status of its
// listeners.
pub fn get_accepted_condition(gateway: &Gateway) -> metav1::Condition {
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
                        "found an addres of type {}, only type IPAddress is supported",
                        addr_type
                    );
                    break;
                }
            }
        }
    }
    accepted
}

// Inspects the provided Gateway and sets the status of its listeners accordingly.
pub fn set_listener_status(gateway: &mut Gateway) -> Result<()> {
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

    let gen = gateway
        .metadata
        .generation
        .ok_or(Error::InvalidConfigError(
            "Gateway generation not found".to_string(),
        ))?;
    for listener in &gateway_spec.listeners {
        let mut final_conditions = vec![];
        let (supported_kinds, conditions) = get_listener_status(listener, gen);
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

pub fn get_service_key(service: &Service) -> Result<NamespacedName> {
    let svc_name = service.meta().name.clone().ok_or(Error::LoadBalancerError(
        "Loadbalancer service name not found".to_string(),
    ))?;
    let svc_ns = service.namespace().ok_or(Error::LoadBalancerError(
        "Loadblancer service namespace not found".to_string(),
    ))?;
    Ok(NamespacedName {
        name: svc_name,
        namespace: svc_ns,
    })
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
            return Some(format!("Unsupported API group: {}", group));
        }
    }
    None
}
