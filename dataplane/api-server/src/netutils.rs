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

// pub fn if_nametoindex(ifname: String) -> Result<u32, Error> {
//     let ifname_c = CString::new(ifname)?;
//     let ifindex = unsafe { libc_if_nametoindex(ifname_c.into_raw()) };
//     Ok(ifindex)
// }

/// Returns an net interface index for a Ipv4 address, like `ip route get to`
pub fn if_index_for_routing_ip(ip_addr: Ipv4Addr) -> Result<u32, Error> {
    // run the linux command "ip route" to get the device's index responsible for
    // routing the given Ipv4 address.
    let mut socket = Socket::new(NETLINK_ROUTE).unwrap();
    let _port_number = socket.bind_auto().unwrap().port_number();
    socket.connect(&SocketAddr::new(0, 0)).unwrap();

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

    let no_device_err: String = format!("no device found to route {}", ip_addr);
    let con_packet_err: String = "construct packet failed".to_string();
    let nl_send_msg_err: String = "netlink send message failed".to_string();
    let nl_recv_msg_err: String = "netlink receive message failed".to_string();

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
    socket
        .send(&buf[..], 0)
        .map_err(|_| Error::msg(nl_send_msg_err))?;

    let mut receive_buffer = vec![0; 4096];
    socket
        .recv(&mut &mut receive_buffer[..], 0)
        .map_err(|_| Error::msg(nl_recv_msg_err))?;

    let bytes = &receive_buffer[..];

    // extract returned RouteNetLinkMessage
    let (_, payload) = <NetlinkMessage<RouteNetlinkMessage>>::deserialize(bytes)
        .map_err(|_| Error::msg(no_device_err.clone()))?
        .into_parts();
    match payload {
        NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewRoute(v)) => {
            if let Some(RouteAttribute::Oif(idex_if)) = v
                .attributes
                .iter()
                .find(|attr| matches!(attr, RouteAttribute::Oif(_)))
            {
                return Ok(*idex_if);
            }
            Err(Error::msg(no_device_err.clone()))
        }

        _ => Err(Error::msg(no_device_err)),
    }
}
