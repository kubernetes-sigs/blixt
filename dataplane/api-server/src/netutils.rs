/*
Copyright 2023 The Kubernetes Authors.

SPDX-License-Identifier: (GPL-2.0-only OR BSD-2-Clause)
*/

use anyhow::Error;
use netlink_packet_core::{NetlinkHeader, NetlinkMessage, NetlinkPayload, NLM_F_REQUEST};
use netlink_packet_route::{
    route::{RouteAddress, RouteAttribute, RouteFlags, RouteHeader, RouteMessage},
    AddressFamily, RouteNetlinkMessage,
};
use netlink_sys::{protocols::NETLINK_ROUTE, Socket, SocketAddr};
use std::net::Ipv4Addr;

const ERR_NO_IFINDEX: &str = "no ifindex found to route";
const ERR_PACKET_CONSTRUCTION: &str = "construct packet failed";

/// Returns a network interface index for an IPv4 address (like the command `ip route get to $IP`)
pub fn if_index_for_routing_ip(ip_addr: Ipv4Addr) -> Result<u32, Error> {
    let socket = Socket::new(NETLINK_ROUTE)?;
    socket.connect(&SocketAddr::new(0, 0))?;

    let mut nl_hdr = NetlinkHeader::default();

    // NNLM_F_REQUEST: Must be set on all request messages
    nl_hdr.flags = NLM_F_REQUEST;

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

    // construct a message packet for netlink and serialize it to send it over the socket
    let mut packet = NetlinkMessage::new(
        nl_hdr,
        NetlinkPayload::from(RouteNetlinkMessage::GetRoute(route_message)),
    );
    packet.finalize();
    let mut buf = vec![0; packet.header.length as usize];
    // check packet
    if buf.len() != packet.buffer_len() {
        return Err(Error::msg(ERR_PACKET_CONSTRUCTION));
    }
    packet.serialize(&mut buf[..]);

    // send the serialized netlink message packet over the socket
    socket.send(&buf[..], 0)?;

    // read all returned messages at once
    let (raw_netlink_message, _) = socket.recv_from_full()?;
    let recv_route_message =
        <NetlinkMessage<RouteNetlinkMessage>>::deserialize(&raw_netlink_message)?;

    // extract returned RouteNetLinkMessage
    if let NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewRoute(message)) =
        recv_route_message.payload
    {
        if let Some(RouteAttribute::Oif(idex_if)) = message
            .attributes
            .iter()
            .find(|attr| matches!(attr, RouteAttribute::Oif(_)))
        {
            return Ok(*idex_if);
        }
    }
    Err(Error::msg(format!("{ERR_NO_IFINDEX} {ip_addr}")))
}
