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

SEC("xdp")
int xdp_prog_func(struct xdp_md *ctx) {
  void *data = (void *)(long)ctx->data;
  void *data_end = (void *)(long)ctx->data_end;

  struct ethhdr *eth = data;
  if (data + sizeof(struct ethhdr) > data_end)
    return XDP_ABORTED;

  if (bpf_ntohs(eth->h_proto) != ETH_P_IP)
    return XDP_PASS;

  struct iphdr *ip = data + sizeof(struct ethhdr);
  if (data + sizeof(struct ethhdr) + sizeof(struct iphdr) > data_end)
    return XDP_ABORTED;

  if (ip->protocol != IPPROTO_UDP)
    return XDP_PASS;

  struct udphdr *udp = data + sizeof(struct ethhdr) + sizeof(struct iphdr);
  if (data + sizeof(struct ethhdr) + sizeof(struct iphdr) + sizeof(struct udphdr) > data_end)
      return XDP_ABORTED;

  bpf_printk("UDP packet received - daddr:%x, port:%d", ip->daddr, bpf_ntohs(udp->dest));

  // TODO: routing

  return XDP_PASS;
}
