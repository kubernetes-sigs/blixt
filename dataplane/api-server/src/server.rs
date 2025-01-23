/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

use std::net::Ipv4Addr;
use std::sync::Arc;

use anyhow::Error;
use aya::maps::{HashMap, MapData, MapError};
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use backends::backends::{Confirmation, InterfaceIndexConfirmation, PodIp, Targets, Vip};

use crate::netutils::if_index_for_routing_ip;
use backends::backends::backends_server::Backends;
use common::{
    Backend, BackendKey, BackendList, ClientKey, LoadBalancerMapping, BACKENDS_ARRAY_CAPACITY,
};

pub struct BackendService {
    backends_map: Arc<Mutex<HashMap<MapData, BackendKey, BackendList>>>,
    gateway_indexes_map: Arc<Mutex<HashMap<MapData, BackendKey, u16>>>,
    tcp_conns_map: Arc<Mutex<HashMap<MapData, ClientKey, LoadBalancerMapping>>>,
}

impl BackendService {
    pub fn new(
        backends_map: HashMap<MapData, BackendKey, BackendList>,
        gateway_indexes_map: HashMap<MapData, BackendKey, u16>,
        tcp_conns_map: HashMap<MapData, ClientKey, LoadBalancerMapping>,
    ) -> BackendService {
        BackendService {
            backends_map: Arc::new(Mutex::new(backends_map)),
            gateway_indexes_map: Arc::new(Mutex::new(gateway_indexes_map)),
            tcp_conns_map: Arc::new(Mutex::new(tcp_conns_map)),
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

        // Delete all entries in our tcp connection tracking map that this backend
        // key was related to. This is needed because the TCPRoute might have been
        // deleted with TCP connection(s) still open, so without the below logic
        // they'll hang around forever.
        // Its better to do this rather than maintain a reverse index because the index
        // would need to be updated with each new connection. With remove being a less
        // frequently used operation, the performance cost is less visible.
        let mut tcp_conns_map = self.tcp_conns_map.lock().await;
        for item in tcp_conns_map
            .iter()
            .collect::<Vec<Result<(ClientKey, LoadBalancerMapping), MapError>>>()
        {
            match item {
                Ok((
                    client_key,
                    LoadBalancerMapping {
                        backend: _,
                        backend_key,
                        tcp_state: _,
                    },
                )) => {
                    if backend_key == key {
                        tcp_conns_map.remove(&client_key)?;
                    };
                }
                Err(err) => return Err(err.into()),
            };
        }
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

        let ifindex = match if_index_for_routing_ip(ip_addr) {
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
                    match if_index_for_routing_ip(ip_addr) {
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
                    "success, vip {}:{} was updated with {} backends",
                    Ipv4Addr::from(vip.ip),
                    vip.port,
                    count,
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
