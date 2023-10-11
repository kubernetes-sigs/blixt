/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

use std::net::Ipv4Addr;
use std::sync::Arc;

use anyhow::Error;
use aya::maps::{HashMap, MapData};
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use crate::backends::backends_server::Backends;
use crate::backends::{Confirmation, InterfaceIndexConfirmation, PodIp, Targets, Vip};
use crate::netutils::{if_name_for_routing_ip, if_nametoindex};
use common::{Backend, BackendKey};

pub struct BackendService {
    bpf_map: Arc<Mutex<HashMap<MapData, BackendKey, Backend>>>,
}

impl BackendService {
    pub fn new(bpf_map: HashMap<MapData, BackendKey, Backend>) -> BackendService {
        BackendService {
            bpf_map: Arc::new(Mutex::new(bpf_map)),
        }
    }

    async fn insert(&self, key: BackendKey, bk: Backend) -> Result<(), Error> {
        let mut bpf_map = self.bpf_map.lock().await;
        bpf_map.insert(key, bk, 0)?;
        Ok(())
    }

    async fn remove(&self, key: BackendKey) -> Result<(), Error> {
        let mut bpf_map = self.bpf_map.lock().await;
        bpf_map.remove(&key)?;
        Ok(())
    }
}

#[tonic::async_trait]
impl Backends for BackendService {
    async fn get_interface_index(
        &self,
        request: Request<PodIp>,
    ) -> Result<Response<InterfaceIndexConfirmation>, Status> {
        let pod = request.into_inner();
        let ip = pod.ip;
        let ip_addr = std::net::Ipv4Addr::from(ip);

        let device = match if_name_for_routing_ip(ip_addr) {
            Ok(device) => device,
            Err(err) => return Err(Status::internal(err.to_string())),
        };

        let ifindex = match if_nametoindex(device) {
            Ok(ifindex) => ifindex,
            Err(err) => return Err(Status::internal(err.to_string())),
        };

        Ok(Response::new(InterfaceIndexConfirmation { ifindex }))
    }

    async fn update(&self, request: Request<Targets>) -> Result<Response<Confirmation>, Status> {
        let targets = request.into_inner();

        let vip = match targets.vip {
            Some(vip) => vip,
            None => return Err(Status::invalid_argument("missing vip ip and port")),
        };

        let target = match targets.target {
            Some(target) => target,
            None => return Err(Status::invalid_argument("missing targets for vip")),
        };

        let key = BackendKey {
            ip: vip.ip,
            port: vip.port,
        };

        let ifindex = match target.ifindex {
            Some(ifindex) => ifindex,
            None => {
                let ip_addr = Ipv4Addr::from(target.daddr);
                let ifname = match if_name_for_routing_ip(ip_addr) {
                    Ok(ifname) => ifname,
                    Err(err) => {
                        return Err(Status::internal(format!(
                            "failed to determine ifname: {}",
                            err
                        )))
                    }
                };

                match if_nametoindex(ifname) {
                    Ok(ifindex) => ifindex,
                    Err(err) => {
                        return Err(Status::internal(format!(
                            "failed to determine ifindex: {}",
                            err
                        )))
                    }
                }
            }
        };

        let bk = Backend {
            daddr: target.daddr,
            dport: target.dport,
            ifindex: ifindex as u16,
        };

        match self.insert(key, bk).await {
            Ok(_) => Ok(Response::new(Confirmation {
                confirmation: format!(
                    "success, vip {}:{} was updated",
                    Ipv4Addr::from(vip.ip),
                    vip.port
                ),
            })),
            Err(err) => Err(Status::internal(format!("failure: {}", err))),
        }
    }

    async fn delete(&self, request: Request<Vip>) -> Result<Response<Confirmation>, Status> {
        let vip = request.into_inner();

        let key = BackendKey {
            ip: vip.ip,
            port: vip.port,
        };

        let addr_ddn = Ipv4Addr::from(vip.ip);

        match self.remove(key).await {
            Ok(()) => Ok(Response::new(Confirmation {
                confirmation: format!("success, vip {}:{} was deleted", addr_ddn, vip.port),
            })),
            Err(err) if err.to_string().contains("syscall failed with code -1") => {
                Ok(Response::new(Confirmation {
                    confirmation: format!("success, vip {}:{} did not exist", addr_ddn, vip.port),
                }))
            }
            Err(err) => Err(Status::internal(format!("failure: {}", err))),
        }
    }
}
