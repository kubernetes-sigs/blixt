/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

use aya_ebpf::{
    bindings::TC_ACT_OK,
    helpers::{bpf_l3_csum_replace, bpf_l4_csum_replace, bpf_skb_store_bytes},
    programs::TcContext,
};
use aya_ebpf_cty::{c_long, c_void};
use aya_log_ebpf::info;
use core::mem;
use network_types::{eth::EthHdr, ip::Ipv4Hdr, tcp::TcpHdr};

use crate::LB_CONNECTIONS;
use common::{ClientKey, LoadBalancerMapping, TCPState};

use memoffset::offset_of;

const IP_CSUM_OFF: u32 = (EthHdr::LEN + offset_of!(Ipv4Hdr, check)) as u32;
const IP_DST_OFF: u32 = (EthHdr::LEN + offset_of!(Ipv4Hdr, dst_addr)) as u32;
const IS_PSEUDO: u64 = 0x10;

// -----------------------------------------------------------------------------
// Helper Functions
// -----------------------------------------------------------------------------

// Gives us raw pointers to a specific offset in the packet
#[inline(always)]
pub unsafe fn ptr_at<T>(ctx: &TcContext, offset: usize) -> Result<*mut T, i64> {
    let start = ctx.data();
    let end = ctx.data_end();
    let len = mem::size_of::<T>();

    if start + offset + len > end {
        return Err(TC_ACT_OK.into());
    }
    Ok((start + offset) as *mut T)
}

// Converts a checksum into u16
#[inline(always)]
pub fn csum_fold_helper(mut csum: u64) -> u16 {
    for _i in 0..4 {
        if (csum >> 16) > 0 {
            csum = (csum & 0xffff) + (csum >> 16);
        }
    }
    !(csum as u16)
}

// Updates the TCP connection's state based on the current phase and the incoming packet's header.
// It returns true if the state transitioned to a different phase.
// Ref: https://en.wikipedia.org/wiki/File:Tcp_state_diagram.png and
// http://www.tcpipguide.com/free/t_TCPConnectionTermination-2.htm
#[inline(always)]
pub fn process_tcp_state_transition(hdr: &TcpHdr, state: &mut TCPState) -> bool {
    let fin = hdr.fin() == 1;
    let ack = hdr.ack() == 1;
    match state {
        TCPState::Established => {
            // At the Established state, a FIN packet moves the state to FinWait1.
            if fin {
                *state = TCPState::FinWait1;
                return true;
            }
        }
        TCPState::FinWait1 => {
            // At the FinWait1 state, a packet with both the FIN and ACK bits set
            // moves the state to TimeWait.
            if fin && ack {
                *state = TCPState::TimeWait;
                return true;
            }
            // At the FinWait1 state, a FIN packet moves the state to Closing.
            if fin {
                *state = TCPState::Closing;
                return true;
            }
            // At the FinWait1 state, an ACK packet moves the state to FinWait2.
            if ack {
                *state = TCPState::FinWait2;
                return true;
            }
        }
        TCPState::FinWait2 => {
            // At the FinWait2 state, an ACK packet moves the state to TimeWait.
            if ack {
                *state = TCPState::TimeWait;
                return true;
            }
        }
        TCPState::Closing => {
            // At the Closing state, an ACK packet moves the state to TimeWait.
            if ack {
                *state = TCPState::TimeWait;
                return true;
            }
        }
        TCPState::TimeWait => {
            if ack {
                *state = TCPState::Closed;
                return true;
            }
        }
        TCPState::Closed => {}
    }
    false
}

// Modifies the map tracking TCP connections based on the current state
// of the TCP connection and the incoming TCP packet's header.
#[inline(always)]
pub fn update_tcp_conns(
    hdr: &TcpHdr,
    client_key: &ClientKey,
    lb_mapping: &mut LoadBalancerMapping,
) -> Result<(), i64> {
    if let Some(ref mut tcp_state) = lb_mapping.tcp_state {
        let transitioned = process_tcp_state_transition(hdr, tcp_state);
        if let TCPState::Closed = tcp_state {
            unsafe {
                return LB_CONNECTIONS.remove(client_key);
            }
        }
        // If the connection has not reached the Closed state yet, but it did transition to a new state,
        // then record the new state.
        if transitioned {
            unsafe {
                return LB_CONNECTIONS.insert(client_key, lb_mapping, 0_u64);
            }
        }
    }
    Ok(())
}

// inspired by https://github.com/torvalds/linux/blob/master/samples/bpf/tcbpf1_kern.c
// update dst_addr in the ip_hdr
// recalculate the checksums
pub fn set_ipv4_ip_dst(ctx: &TcContext, l4_csum_offset: u32, old_ip: &u32, new_dip: u32) -> c_long {
    let mut ret: c_long;
    unsafe {
        ret = bpf_l4_csum_replace(
            ctx.skb.skb,
            l4_csum_offset,
            *old_ip as u64,
            new_dip as u64,
            IS_PSEUDO | (mem::size_of_val(&new_dip) as u64),
        );
    }
    if ret != 0 {
        info!(
            ctx,
            "Failed to update the TCP checksum after modifying the destination IP"
        );
        return ret;
    }

    unsafe {
        ret = bpf_l3_csum_replace(
            ctx.skb.skb,
            IP_CSUM_OFF,
            *old_ip as u64,
            new_dip as u64,
            mem::size_of_val(&new_dip) as u64,
        );
    }
    if ret != 0 {
        info!(
            ctx,
            "Failed to update the IP header checksum after modifying the destination IP"
        );
        return ret;
    }

    unsafe {
        ret = bpf_skb_store_bytes(
            ctx.skb.skb,
            IP_DST_OFF,
            &new_dip as *const u32 as *const c_void,
            mem::size_of_val(&new_dip) as u32,
            0,
        );
    }
    if ret != 0 {
        info!(
            ctx,
            "Failed to update the destination IP address in the packet header"
        );
        return ret;
    }

    ret
}

// update destination port in the tcp_hdr
// recalculate the checksums
pub fn set_ipv4_dest_port(
    ctx: &TcContext,
    l4_csum_offset: u32,
    old_port: &u16,
    new_port: u16,
) -> c_long {
    let mut ret: c_long;
    unsafe {
        ret = bpf_l4_csum_replace(
            ctx.skb.skb,
            l4_csum_offset,
            *old_port as u64,
            new_port as u64,
            mem::size_of_val(&new_port) as u64,
        );
    }
    if ret != 0 {
        info!(
            ctx,
            "Failed to update the TCP checksum after modifying the destination port"
        );
        return ret;
    }

    unsafe {
        ret = bpf_skb_store_bytes(
            ctx.skb.skb,
            l4_csum_offset,
            &new_port as *const u16 as *const c_void,
            mem::size_of_val(&new_port) as u32,
            0,
        );
    }
    if ret != 0 {
        info!(
            ctx,
            "Failed to update the destination port in the packet header"
        );
        return ret;
    }

    ret
}
