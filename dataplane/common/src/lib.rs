/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

#![no_std]

pub const BACKENDS_ARRAY_CAPACITY: usize = 128;
pub const BPF_MAPS_CAPACITY: u32 = 128;

#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
pub struct Backend {
    pub daddr: u32,
    pub dport: u32,
    pub ifindex: u16,
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for Backend {}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct BackendKey {
    pub ip: u32,
    pub port: u32,
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for BackendKey {}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct BackendList {
    pub backends: [Backend; BACKENDS_ARRAY_CAPACITY],
    // backends_len is the length of the backends array
    pub backends_len: u16,
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for BackendList {}
