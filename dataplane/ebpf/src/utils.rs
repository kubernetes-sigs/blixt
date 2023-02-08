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

// Produces an IPv4 address as a [u8;4] which is easy to print out in
// dot-decimal notation using the info!() macro.
//
// TODO: use a type alias and implement print formatting for this?
#[inline(always)]
pub fn ip_from_int(ip: u32) -> [u8; 4] {
    let mut addr: [u8; 4] = [0; 4];

    addr[0] = ((ip >> 0) & 0xFF) as u8;
    addr[1] = ((ip >> 8) & 0xFF) as u8;
    addr[2] = ((ip >> 16) & 0xFF) as u8;
    addr[3] = ((ip >> 24) & 0xFF) as u8;

    addr
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
