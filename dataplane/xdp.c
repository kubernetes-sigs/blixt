// +build ignore

#include <linux/bpf_common.h>
#include <linux/if_ether.h>
#include <linux/in.h>
#include <linux/ip.h>
#include <linux/udp.h>
#include <linux/bpf.h>
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_endian.h>

char __license[] SEC("license") = "GPL";

#ifndef memcpy
 #define memcpy(dest, src, n) __builtin_memcpy((dest), (src), (n))
#endif

#define MAX_BACKENDS 128
#define MAX_UDP_LENGTH 1480

static __always_inline void ip_from_int(__u32 *buf, __be32 ip) {
    buf[0] = (ip >> 0 ) & 0xFF;
    buf[1] = (ip >> 8 ) & 0xFF;
    buf[2] = (ip >> 16 ) & 0xFF;
    buf[3] = (ip >> 24 ) & 0xFF;
}

static __always_inline void bpf_printk_ip(__be32 ip) {
    __u32 ip_parts[4];
    ip_from_int((__u32 *)&ip_parts, ip);
    bpf_printk("%d.%d.%d.%d", ip_parts[0], ip_parts[1], ip_parts[2], ip_parts[3]);
}

static __always_inline __u16 csum_fold_helper(__u64 csum) {
    int i;
#pragma unroll
    for (i = 0; i < 4; i++)
    {
        if (csum >> 16)
            csum = (csum & 0xffff) + (csum >> 16);
    }
    return ~csum;
}

static __always_inline __u16 iph_csum(struct iphdr *iph) {
    iph->check = 0;
    unsigned long long csum = bpf_csum_diff(0, 0, (unsigned int *)iph, sizeof(struct iphdr), 0);
    return csum_fold_helper(csum);
}

static __always_inline __u16 udp_checksum(struct iphdr *ip, struct udphdr * udp, void * data_end) {
    udp->check = 0;

    __u16 csum = 0;
    __u16 *buf = (__u16*)udp;

    csum += ip->saddr;
    csum += ip->saddr >> 16;
    csum += ip->daddr;
    csum += ip->daddr >> 16;
    csum += (__u16)ip->protocol << 8;
    csum += udp->len;

    for (int i = 0; i < MAX_UDP_LENGTH; i += 2) {
        if ((void *)(buf + 1) > data_end) {
            break;
        }
        csum += *buf;
        buf++;
    }

    if ((void *)buf + 1 <= data_end) {
        csum += *(__u8 *)buf;
    }

   csum = ~csum;
   return csum;
}

struct backend {
    __u32 saddr;
    __u32 daddr;
    __u8 hwaddr[6];
    __u16 ifindex;
};

struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, MAX_BACKENDS);
    __type(key, __u32);
    __type(value, struct backend);
} backends SEC(".maps");

SEC("xdp")
int xdp_prog_func(struct xdp_md *ctx) {
  // ---------------------------------------------------------------------------
  // Initialize
  // ---------------------------------------------------------------------------

  void *data = (void *)(long)ctx->data;
  void *data_end = (void *)(long)ctx->data_end;

  struct ethhdr *eth = data;
  if (data + sizeof(struct ethhdr) > data_end) {
    bpf_printk("ABORTED: bad ethhdr!");
    return XDP_ABORTED;
  }

  if (bpf_ntohs(eth->h_proto) != ETH_P_IP) {
    bpf_printk("PASS: not IP protocol!");
    return XDP_PASS;
  }

  struct iphdr *ip = data + sizeof(struct ethhdr);
  if (data + sizeof(struct ethhdr) + sizeof(struct iphdr) > data_end) {
    bpf_printk("ABORTED: bad iphdr!");
    return XDP_ABORTED;
  }

  if (ip->protocol != IPPROTO_UDP)
    return XDP_PASS;

  struct udphdr *udp = data + sizeof(struct ethhdr) + sizeof(struct iphdr);
  if (data + sizeof(struct ethhdr) + sizeof(struct iphdr) + sizeof(struct udphdr) > data_end) {
    bpf_printk("ABORTED: bad udphdr!");
    return XDP_ABORTED;
  }

  bpf_printk("UDP packet received - daddr:%x, port:%d", ip->daddr, bpf_ntohs(udp->dest));

  // ---------------------------------------------------------------------------
  // Routing
  // ---------------------------------------------------------------------------

  __u32 original_dest_ip = ip->daddr;

  struct backend *bk;
  bk = bpf_map_lookup_elem(&backends, &original_dest_ip);
  if (!bk) {
      bpf_printk("no backends for ip %x", original_dest_ip);
      return XDP_PASS;
  }

  bpf_printk("got UDP traffic, source address:");
  bpf_printk_ip(ip->saddr);
  bpf_printk("destination address:");
  bpf_printk_ip(ip->daddr);

  ip->saddr = bk->saddr;
  ip->daddr = bk->daddr;

  bpf_printk("updated saddr to:");
  bpf_printk_ip(ip->saddr);
  bpf_printk("updated daddr to:");
  bpf_printk_ip(ip->daddr);

  memcpy(eth->h_source, eth->h_dest, sizeof(eth->h_source));
  bpf_printk("new source hwaddr %x:%x:%x:%x:%x:%x", eth->h_source[0], eth->h_source[1], eth->h_source[2], eth->h_source[3], eth->h_source[4], eth->h_source[5]);

  memcpy(eth->h_dest, bk->hwaddr, sizeof(eth->h_dest));
  bpf_printk("new dest hwaddr %x:%x:%x:%x:%x:%x", eth->h_dest[0], eth->h_dest[1], eth->h_dest[2], eth->h_dest[3], eth->h_dest[4], eth->h_dest[5]);

  ip->check = iph_csum(ip);
  udp->check = udp_checksum(ip, udp, data_end);

  bpf_printk("destination interface index %d", bk->ifindex);

  return bpf_redirect(bk->ifindex, 0);
}

SEC("xdp")
int bpf_redirect_placeholder(struct xdp_md *ctx) {
    bpf_printk("received a packet on dest interface");
    return XDP_PASS;
}
