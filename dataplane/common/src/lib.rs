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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct ClientKey {
    pub ip: u32,
    pub port: u32,
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for ClientKey {}

// TCPState contains variants that represent the current phase of the TCP connection at a point in
// time during the connection's termination.
#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
pub enum TCPState {
    #[default]
    Established,
    FinWait1,
    FinWait2,
    Closing,
    TimeWait,
    Closed,
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for TCPState {}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct TCPBackend {
    pub backend: Backend,
    pub backend_key: BackendKey,
    pub state: TCPState,
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for TCPBackend {}
