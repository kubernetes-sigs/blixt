use std::net::Ipv4Addr;
use std::sync::Arc;
use std::vec::Vec;

use anyhow::Error;
use aya::maps::{HashMap, MapRefMut};
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use crate::backends::backends_server::Backends;
use crate::backends::{Confirmation, InterfaceIndexConfirmation, PodIp, Targets, Vip};
use crate::netutils::{if_name_for_routing_ip, if_nametoindex};
use common::{Backend, BackendKey, BackendsList, BACKENDS_ARRAY_LENGTH};

pub struct BackendService {
    bpf_map: Arc<Mutex<HashMap<MapRefMut, BackendKey, BackendsList>>>,
}

impl BackendService {
    pub fn new(bpf_map: HashMap<MapRefMut, BackendKey, BackendsList>) -> BackendService {
        BackendService {
            bpf_map: Arc::new(Mutex::new(bpf_map)),
        }
    }

    async fn insert(&self, key: BackendKey, bks: Vec<Backend>) -> Result<(), Error> {
        let mut bpf_map = self.bpf_map.lock().await;
        let mut backends_list: BackendsList;
        let mut i = 0;

        match bpf_map.get(&key, 0) {
            Ok(v) => {
                backends_list = v.clone();
                for bk in bks.iter() {
                    backends_list.backends[i] = *bk;
                    i += 1;
                }
                backends_list.n_elements = bks.len();
            }
            Err(_) => {
                backends_list = BackendsList {
                    backends: [Backend{
                        daddr: 0,
                        dport: 0,
                        ifindex: 0,
                    }; BACKENDS_ARRAY_LENGTH],
                    index: 0,
                    n_elements: 0,
                };
                for bk in bks.iter() {
                    backends_list.backends[i] = *bk;
                    i += 1;
                }
                backends_list.n_elements = bks.len();
            }
        };
        bpf_map.insert(key, backends_list, 0)?;
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

        if targets.targets.len() == 0 {
            return Err(Status::invalid_argument("missing targets for vip"));
        }

        let key = BackendKey {
            ip: vip.ip,
            port: vip.port,
        };

        let mut bks: Vec<Backend> = Vec::new();

        for target in targets.targets.iter() {
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
            bks.push(Backend {
                daddr: target.daddr,
                dport: target.dport,
                ifindex: ifindex as u16,
            });
        }


        match self.insert(key, bks).await {
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
