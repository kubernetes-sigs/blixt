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
use common::{Backend, BackendKey, BackendList, BACKENDS_ARRAY_CAPACITY};

pub struct BackendService {
    backends_map: Arc<Mutex<HashMap<MapData, BackendKey, BackendList>>>,
    gateway_indexes_map: Arc<Mutex<HashMap<MapData, BackendKey, u16>>>,
}

impl BackendService {
    pub fn new(
        backends_map: HashMap<MapData, BackendKey, BackendList>,
        gateway_indexes_map: HashMap<MapData, BackendKey, u16>,
    ) -> BackendService {
        BackendService {
            backends_map: Arc::new(Mutex::new(backends_map)),
            gateway_indexes_map: Arc::new(Mutex::new(gateway_indexes_map)),
        }
    }

    async fn insert(&self, key: BackendKey, bks: BackendList) -> Result<(), Error> {
        let mut backends_map = self.backends_map.lock().await;
        backends_map.insert(key, bks, 0)?;
        Ok(())
    }

    async fn insert_and_reset_index(&self, key: BackendKey, bks: BackendList) -> Result<(), Error> {
        self.insert(key, bks).await?;
        let mut gateway_indexes_map = self.gateway_indexes_map.lock().await;
        gateway_indexes_map.insert(key, 0, 0)?;
        Ok(())
    }

    async fn remove(&self, key: BackendKey) -> Result<(), Error> {
        let mut backends_map = self.backends_map.lock().await;
        backends_map.remove(&key)?;
        let mut gateway_indexes_map = self.gateway_indexes_map.lock().await;
        gateway_indexes_map.remove(&key)?;
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

        let key = BackendKey {
            ip: vip.ip,
            port: vip.port,
        };
        let mut backends: [Backend; BACKENDS_ARRAY_CAPACITY] =
            [Backend::default(); BACKENDS_ARRAY_CAPACITY];
        let mut count: u16 = 0;
        let backend_targets = targets.targets;

        for backend_target in backend_targets {
            let ifindex = match backend_target.ifindex {
                Some(ifindex) => ifindex,
                None => {
                    let ip_addr = Ipv4Addr::from(backend_target.daddr);
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

            if (count as usize) < BACKENDS_ARRAY_CAPACITY {
                let bk = Backend {
                    daddr: backend_target.daddr,
                    dport: backend_target.dport,
                    ifindex: ifindex as u16,
                };
                backends[count as usize] = bk;
                count += 1;
            } else {
                return Err(Status::resource_exhausted(
                    "BPF map value capacity exceeded, only 128 backends supported per Gateway",
                ));
            }
        }

        let backend_list = BackendList {
            backends,
            backends_len: count,
        };
        match self.insert_and_reset_index(key, backend_list).await {
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
