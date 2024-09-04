/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

use anyhow::Error;
use netlink_packet_core::{
    NetlinkHeader, NetlinkMessage, NetlinkPayload, NLM_F_DUMP_FILTERED, NLM_F_REQUEST,
};
use netlink_packet_route::{
    route::{RouteAddress, RouteAttribute, RouteFlags, RouteHeader, RouteMessage},
    AddressFamily, RouteNetlinkMessage,
};
use netlink_sys::{protocols::NETLINK_ROUTE, Socket, SocketAddr};
use std::net::Ipv4Addr;

/// Returns an network interface index for a Ipv4 address (like the command `ip route get to $IP`)
pub fn if_index_for_routing_ip(ip_addr: Ipv4Addr) -> Result<u32, Error> {
    let mut socket = Socket::new(NETLINK_ROUTE)?;
    let _port_number = socket.bind_auto()?.port_number();
    socket.connect(&SocketAddr::new(0, 0))?;

    let mut nl_hdr = NetlinkHeader::default();
    nl_hdr.flags = NLM_F_REQUEST | NLM_F_DUMP_FILTERED;

    // construct RouteMessage
    let route_header = RouteHeader {
        address_family: AddressFamily::Inet,
        flags: RouteFlags::LookupTable,
        destination_prefix_length: 32,
        table: RouteHeader::RT_TABLE_MAIN,
        ..Default::default()
    };
    let route_attribute = RouteAttribute::Destination(RouteAddress::Inet(ip_addr));
    let mut route_message = RouteMessage::default();
    route_message.attributes = vec![route_attribute];
    route_message.header = route_header;

    let no_ifindex_err = format!("no ifindex found to route {}", ip_addr);
    let con_packet_err = "construct packet failed".to_string();

    let mut packet = NetlinkMessage::new(
        nl_hdr,
        NetlinkPayload::from(RouteNetlinkMessage::GetRoute(route_message)),
    );
    packet.finalize();
    let mut buf = vec![0; packet.header.length as usize];
    // check packet
    if buf.len() != packet.buffer_len() {
        return Err(Error::msg(con_packet_err));
    }
    packet.serialize(&mut buf[..]);
    socket.send(&buf[..], 0)?;

    let mut receive_buffer = vec![0; 4096];
    socket.recv(&mut &mut receive_buffer[..], 0)?;

    let bytes = &receive_buffer[..];

    // extract returned RouteNetLinkMessage
    let (_, payload) = <NetlinkMessage<RouteNetlinkMessage>>::deserialize(bytes)?.into_parts();
    match payload {
        NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewRoute(v)) => {
            if let Some(RouteAttribute::Oif(idex_if)) = v
                .attributes
                .iter()
                .find(|attr| matches!(attr, RouteAttribute::Oif(_)))
            {
                return Ok(*idex_if);
            }
            Err(Error::msg(no_ifindex_err.clone()))
        }

        _ => Err(Error::msg(no_ifindex_err)),
    }
}
