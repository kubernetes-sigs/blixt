# Some helpful hints for debugging this XDP program

## Tracing XDP redirect (on first interface where main XDP program is attached)

(TODO finish tracing the XDP path through the kernel)
1. Entry at `xdp_do_redirect`
    - Frags Don't work `xdp_buff_has_frags`
    - If map == XSKMAP -> `__xdp_do_redirect_xsk`
    - Returns `__xdp_do_redirect_frame`

2. Entry `__xdp_do_redirect_frame` (Can't trace internal functions?)


## Tracing Once packet meets host end of veth 

(TODO finish tracing the XDP path through the kernel)
__netif_receive_skb_core 


## Debugging UDP Checksum issues

We can use TCP dump see if cksum's are correct once the packets reach the container:

```bash
sudo tcpdump -vvv -i <Container Interface> -neep udp`
```

`__sum16 __skb_checksum_complete(struct sk_buff *skb)` is the name of the kernel 
function which will actually check the cksum, it can be tracked with `bpftrace` 
and the following kprobe: 

```bash
kretprobe:__skb_checksum_complete
{ 
    printf("skb_checksum_complete returned: %x\n", retval);   
}
```

## Manually Calculating UDP Checksums

A UDP cksum is calculated with the following: 

```bash
1's Complement {
    Source IP +
    Destination IP +
    17 (0x0011 - UDP protocol code) +
    UDP Packet Length + Source Port +
    Destination Port +
    UDP Packet Length +
    Data
}
```

A Raw TCPdump packet is shown below:
```bash
13:23:15.756911 06:56:87:ec:fd:1f > 86:ad:33:29:ff:5e, ethertype IPv4 (0x0800), length 60: (tos 0x0, ttl 57, id 20891, offset 0, flags [DF], proto UDP (17), length 33)
    10.8.125.12.58980 > 192.168.10.2.sapv1: [bad udp cksum 0xd301 -> 0xaf43!] UDP, length 5
        0x0000:  86ad 3329 ff5e 0656 87ec fd1f 0800 4500
        0x0010:  0021 519b 4000 3911 9e72 0a08 7d0c c0a8
        0x0020:  0a02 e664 2693 000d d301 7465 7374 0a00
        0x0030:  0000 0000 d2f2 935d 0000 0000
```

Using this along with our knowledge of a UDP packet we can quickly and manually 
calculate the cksum like so: 

```bash
0x0a08  Src IP octet 1
0x7d0c  Src IP octet 2
0xc0a8  Dst IP octet 1
0x0a02  Dst IP octet 2
0x0011  Proto
0x000d  Length
0xe664  Src Port
0x2693  Dst Port
0x000d  Length
0x7465  Data
0x7374  Data
0x0a00  Data
+
-------------
50bc -> 1's compliment = af43
```

To play with this same raw data in wireshark we can use the text from the hex dump
and convert it to the following format.  With this in a file you can then
"Import from hex dump" in wireshark.

```bash
13:23:15
0000 86 ad 33 29 ff 5e 06 56 87 ec fd 1f 08 00 45 00
0010 00 21 51 9b 40 00 39 11 9e 72 0a 08 7d 0c c0 a8
0020 0a 02 e6 64 26 93 00 0d d3 01 74 65 73 74 0a 00
0030 00 00 00 00 d2 f2 93 5d 00 00 00 00
```

![Above Raw packet shown in wireshark](./wireshark.png)

## Tracing Non XDP stack (native kernel) with PWRU

Cilium's [PWRU](https://github.com/cilium/pwru) is a great tool for tracing packets
as they make their way through the linux kernel. It is limited in the fact that it
doesn't really track the XDP stack currently, however it's still super helpful 
for debugging other issues. 

### Working Trace (manually re-writing Cksums)

```bash
0xffff96d3956d4f00      8    [ksoftirqd/8]         udp4_gro_receive
0xffff96d3956d4f00      8    [ksoftirqd/8]          udp_gro_receive
0xffff96d3956d4f00      8    [ksoftirqd/8]   skb_defer_rx_timestamp
0xffff96d3956d4f00      8    [ksoftirqd/8]              tpacket_rcv
0xffff96d3956d4f00      8    [ksoftirqd/8]                 skb_push
0xffff96d3956d4f00      8    [ksoftirqd/8]    tpacket_get_timestamp
0xffff96d3956d4f00      8    [ksoftirqd/8]              consume_skb
0xffff96d3956d4f00     10             [nc]          skb_consume_udp
0xffff96d3956d4f00     10             [nc]          skb_consume_udp
0xffff96d3956d4f00     10             [nc]  __consume_stateless_skb
0xffff96d3956d4f00     10             [nc]         skb_release_data
0xffff96d3956d4f00     10             [nc]            skb_free_head
0xffff96d3956d4f00     10             [nc]             kfree_skbmem
0xffff96d3956d4f00      8    [ksoftirqd/8]              ip_rcv_core
0xffff96d3956d4f00      8    [ksoftirqd/8]     pskb_trim_rcsum_slow
0xffff96d3956d4f00      8    [ksoftirqd/8]       udp_v4_early_demux
0xffff96d3956d4f00      8    [ksoftirqd/8]     ip_route_input_noref
0xffff96d3956d4f00      8    [ksoftirqd/8]       ip_route_input_rcu
0xffff96d3956d4f00      8    [ksoftirqd/8]      ip_route_input_slow
0xffff96d3956d4f00      8    [ksoftirqd/8]      fib_validate_source
0xffff96d3956d4f00      8    [ksoftirqd/8]    __fib_validate_source
0xffff96d3956d4f00      8    [ksoftirqd/8]         ip_local_deliver
0xffff96d3956d4f00      8    [ksoftirqd/8]  ip_local_deliver_finish
0xffff96d3956d4f00      8    [ksoftirqd/8]  ip_protocol_deliver_rcu
0xffff96d3956d4f00      8    [ksoftirqd/8]        raw_local_deliver
0xffff96d3956d4f00      8    [ksoftirqd/8]                  udp_rcv
0xffff96d3956d4f00      8    [ksoftirqd/8]           __udp4_lib_rcv
0xffff96d3956d4f00      8    [ksoftirqd/8]  __skb_checksum_complete
0xffff96d3956d4f00      8    [ksoftirqd/8]      udp_unicast_rcv_skb
0xffff96d3956d4f00      8    [ksoftirqd/8]        udp_queue_rcv_skb
0xffff96d3956d4f00      8    [ksoftirqd/8]    udp_queue_rcv_one_skb
0xffff96d3956d4f00      8    [ksoftirqd/8]       sk_filter_trim_cap
0xffff96d3956d4f00      8    [ksoftirqd/8]    security_sock_rcv_skb
0xffff96d3956d4f00      8    [ksoftirqd/8] selinux_socket_sock_rcv_skb
0xffff96d3956d4f00      8    [ksoftirqd/8] selinux_sock_rcv_skb_compat
0xffff96d3956d4f00      8    [ksoftirqd/8] selinux_netlbl_sock_rcv_skb
0xffff96d3956d4f00      8    [ksoftirqd/8] selinux_xfrm_sock_rcv_skb
0xffff96d3956d4f00      8    [ksoftirqd/8] bpf_lsm_socket_sock_rcv_skb
```

### Working Trace (ignoring cksums i.e setting to 0)

```bash
0xffff96d35c18f000      8        [<empty>]         udp4_gro_receive
0xffff96d35c18f000      8        [<empty>]          udp_gro_receive
0xffff96d35c18f000      8        [<empty>]   skb_defer_rx_timestamp
0xffff96d35c18f000      8        [<empty>]              tpacket_rcv
0xffff96d35c18f000      8        [<empty>]                 skb_push
0xffff96d35c18f000      8        [<empty>]    tpacket_get_timestamp
0xffff96d35c18f000     10             [nc]          skb_consume_udp
0xffff96d35c18f000     10             [nc]          skb_consume_udp
0xffff96d35c18f000     10             [nc]  __consume_stateless_skb
0xffff96d35c18f000     10             [nc]         skb_release_data
0xffff96d35c18f000     10             [nc]            skb_free_head
0xffff96d35c18f000     10             [nc]             kfree_skbmem
0xffff96d35c18f000      8        [<empty>]              consume_skb
0xffff96d35c18f000      8        [<empty>]              ip_rcv_core
0xffff96d35c18f000      8        [<empty>]     pskb_trim_rcsum_slow
0xffff96d35c18f000      8        [<empty>]       udp_v4_early_demux
0xffff96d35c18f000      8        [<empty>]     ip_route_input_noref
0xffff96d35c18f000      8        [<empty>]       ip_route_input_rcu
0xffff96d35c18f000      8        [<empty>]      ip_route_input_slow
0xffff96d35c18f000      8        [<empty>]      fib_validate_source
0xffff96d35c18f000      8        [<empty>]    __fib_validate_source
0xffff96d35c18f000      8        [<empty>]         ip_local_deliver
0xffff96d35c18f000      8        [<empty>]  ip_local_deliver_finish
0xffff96d35c18f000      8        [<empty>]  ip_protocol_deliver_rcu
0xffff96d35c18f000      8        [<empty>]        raw_local_deliver
0xffff96d35c18f000      8        [<empty>]                  udp_rcv
0xffff96d35c18f000      8        [<empty>]           __udp4_lib_rcv # ----> No CKSUM so we don't call __skb_checksum_complete
0xffff96d35c18f000      8        [<empty>]      udp_unicast_rcv_skbx_
0xffff96d35c18f000      8        [<empty>]        udp_queue_rcv_skb
0xffff96d35c18f000      8        [<empty>]    udp_queue_rcv_one_skb
0xffff96d35c18f000      8        [<empty>]       sk_filter_trim_cap
0xffff96d35c18f000      8        [<empty>]    security_sock_rcv_skb
0xffff96d35c18f000      8        [<empty>] selinux_socket_sock_rcv_skb
0xffff96d35c18f000      8        [<empty>] selinux_sock_rcv_skb_compat
0xffff96d35c18f000      8        [<empty>] selinux_netlbl_sock_rcv_skb
0xffff96d35c18f000      8        [<empty>] selinux_xfrm_sock_rcv_skb
0xffff96d35c18f000      8        [<empty>] bpf_lsm_socket_sock_rcv_skb
0xffff96d35c18f000      8        [<empty>]           skb_pull_rcsum
```