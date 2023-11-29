/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

use core::mem;

use aya_bpf::{
    bindings::TC_ACT_PIPE,
    helpers::{bpf_csum_diff, bpf_redirect_neigh},
    programs::TcContext,
};
use aya_log_ebpf::{debug, info};
use network_types::{ip::Ipv4Hdr, eth::EthHdr, udp::UdpHdr};

use crate::{
    utils::{csum_fold_helper, ptr_at, ETH_HDR_LEN, IP_HDR_LEN},
    BACKENDS, BLIXT_CONNTRACK, GATEWAY_INDEXES,
};
use common::{BackendKey, BACKENDS_ARRAY_CAPACITY};

pub fn handle_udp_ingress(ctx: TcContext) -> Result<i32, i64> {
    
    let ip_hdr: *mut Ipv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN)? };


    let udp_hdr: *mut UdpHdr =
                unsafe { ptr_at(&ctx, EthHdr::LEN + Ipv4Hdr::LEN) }?;

    let original_daddr = unsafe { (*ip_hdr).dst_addr };

    let key = BackendKey {
        ip: u32::from_be(original_daddr),
        port: (u16::from_be(unsafe { (*udp_hdr).dest })) as u32,
    };
    let backend_list = unsafe { BACKENDS.get(&key) }.ok_or(TC_ACT_PIPE)?;
    let backend_index = unsafe { GATEWAY_INDEXES.get(&key) }.ok_or(TC_ACT_PIPE)?;

    info!(
        &ctx,
        "Received a UDP packet destined for svc ip: {:i} at Port: {} ",
        u32::from_be(unsafe { (*ip_hdr).dst_addr }),
        u16::from_be(unsafe { (*udp_hdr).dest })
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
                "Failed to find backend in backends_list at index {}, using 0th index; backends_len: {} ",
                *backend_index,
                backend_list.backends_len
            )
        }
    }

    unsafe {
        BLIXT_CONNTRACK.insert(
            &(*ip_hdr).src_addr,
            &(original_daddr, (*udp_hdr).dest as u32),
            0 as u64,
        )?;
        (*ip_hdr).dst_addr = backend.daddr.to_be();
    };

    if (ctx.data() + ETH_HDR_LEN + IP_HDR_LEN) > ctx.data_end() {
        info!(&ctx, "Iphdr is out of bounds");
        return Ok(TC_ACT_PIPE);
    }

    // Calculate l3 cksum
    // TODO(astoycos) use l3_cksum_replace instead
    unsafe { (*ip_hdr).check = 0 };
    let full_cksum = unsafe {
        bpf_csum_diff(
            mem::MaybeUninit::zeroed().assume_init(),
            0,
            ip_hdr as *mut u32,
            IP_HDR_LEN as u32,
            0,
        )
    } as u64;
    unsafe { (*ip_hdr).check = csum_fold_helper(full_cksum) };

    // Update destination port
    unsafe { (*udp_hdr).dest = (backend.dport as u16).to_be() };
    // Kernel allows UDP packet with unset checksums
    unsafe { (*udp_hdr).check = 0 };

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
        GATEWAY_INDEXES.insert(&key, &next, 0 as u64)?;
    }

    info!(&ctx, "redirect action: {}", action);

    Ok(action as i32)
}
