/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

use core::mem;

use aya_bpf::{bindings::TC_ACT_OK, programs::TcContext};

use crate::bindings::{ethhdr, iphdr};

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------

pub const ETH_P_IP: u16 = 0x0800;

pub const IPPROTO_TCP: u8 = 6;
pub const IPPROTO_UDP: u8 = 17;
pub const IPPROTO_ICMP: u8 = 1;

pub const ETH_HDR_LEN: usize = mem::size_of::<ethhdr>();
pub const IP_HDR_LEN: usize = mem::size_of::<iphdr>();

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
    return !(csum as u16);
}
