/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

#![allow(static_mut_refs)]

use core::mem;

use aya_ebpf::{bindings::TC_ACT_PIPE, helpers::bpf_redirect_neigh, programs::TcContext};
use aya_log_ebpf::{debug, info};

use memoffset::offset_of;
use network_types::{eth::EthHdr, ip::Ipv4Hdr, udp::UdpHdr};

use crate::{
    utils::{ptr_at, set_ipv4_dest_port, set_ipv4_ip_dst},
    BACKENDS, GATEWAY_INDEXES, LB_CONNECTIONS,
};
use common::{BackendKey, ClientKey, LoadBalancerMapping, BACKENDS_ARRAY_CAPACITY};

const UDP_CSUM_OFF: u32 = (EthHdr::LEN + Ipv4Hdr::LEN + offset_of!(UdpHdr, check)) as u32;

pub fn handle_udp_ingress(ctx: TcContext) -> Result<i32, i64> {
    let ip_hdr: *mut Ipv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN)? };

    let udp_header_offset = EthHdr::LEN + Ipv4Hdr::LEN;

    let udp_hdr: *mut UdpHdr = unsafe { ptr_at(&ctx, udp_header_offset) }?;

    let original_daddr = unsafe { (*ip_hdr).dst_addr };
    let original_dport = unsafe { (*udp_hdr).dest };

    let backend_key = BackendKey {
        ip: u32::from_be(original_daddr),
        port: (u16::from_be(original_dport)) as u32,
    };
    let backend_list = unsafe { BACKENDS.get(&backend_key) }.ok_or(TC_ACT_PIPE)?;
    let backend_index = unsafe { GATEWAY_INDEXES.get(&backend_key) }.ok_or(TC_ACT_PIPE)?;

    info!(
        &ctx,
        "Received a UDP packet destined for svc ip: {:i} at Port: {} ",
        backend_key.ip,
        backend_key.port as u16,
    );
    debug!(&ctx, "Destination backend index: {}", *backend_index);
    debug!(&ctx, "Backends length: {}", backend_list.backends_len);

    // this check asserts that we don't use a "zero-value" Backend
    if backend_list.backends_len <= *backend_index {
        return Ok(TC_ACT_PIPE);
    }
    // this check is to make the verifier happy
    if *backend_index as usize >= BACKENDS_ARRAY_CAPACITY {
        return Ok(TC_ACT_PIPE);
    }

    let mut backend = backend_list.backends[0];
    match backend_list.backends.get(*backend_index as usize) {
        Some(bk) => backend = *bk,
        None => {
            debug!(
                &ctx,
                "Failed to find backend in backends_list at index {}, falling back to 0th index; backends_len: {} ",
                *backend_index,
                backend_list.backends_len
            )
        }
    }

    unsafe {
        // DNAT the ip address
        (*ip_hdr).dst_addr = backend.daddr.to_be();
        // DNAT the port
        (*udp_hdr).dest = (backend.dport as u16).to_be();

        // Record the packet's source and destination in our connection tracking map.
        let client_key = ClientKey {
            ip: u32::from_be((*ip_hdr).src_addr),
            // The only reason we're tracking UDP packets is to be able to allow ICMP egress
            // traffic. Since ICMP is a L3 protocol, an ICMP packet's header does not have access to
            // the UDP port and operates solely based on the IP address.
            port: 0,
        };
        let lb_mapping = LoadBalancerMapping {
            backend,
            backend_key,
            tcp_state: None,
        };
        LB_CONNECTIONS.insert(&client_key, &lb_mapping, 0_u64)?;
    };

    if (ctx.data() + EthHdr::LEN + Ipv4Hdr::LEN) > ctx.data_end() {
        info!(&ctx, "Iphdr is out of bounds");
        return Ok(TC_ACT_PIPE);
    }

    let backend_ip = backend.daddr.to_be();
    let ret = set_ipv4_ip_dst(&ctx, UDP_CSUM_OFF, &original_daddr, backend_ip);
    if ret != 0 {
        return Ok(TC_ACT_PIPE);
    }

    let backend_port = (backend.dport as u16).to_be();
    let ret = set_ipv4_dest_port(&ctx, UDP_CSUM_OFF, &original_dport, backend_port);
    if ret != 0 {
        return Ok(TC_ACT_PIPE);
    }

    let action = unsafe {
        bpf_redirect_neigh(
            backend.ifindex as u32,
            mem::MaybeUninit::zeroed().assume_init(),
            0,
            0,
        )
    };

    // move the index to the next backend in our list
    let mut next = *backend_index + 1;
    if next >= backend_list.backends_len {
        next = 0;
    }
    unsafe {
        GATEWAY_INDEXES.insert(&backend_key, &next, 0_u64)?;
    }

    info!(&ctx, "redirect action: {}", action);

    Ok(action as i32)
}
