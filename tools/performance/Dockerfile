FROM alpine

LABEL org.opencontainers.image.source=https://github.com/kubernetes-sigs/blixt

RUN apk --no-cache add iperf3

RUN adduser -D -s /bin/sh iperf3

USER iperf3

ENTRYPOINT ["iperf3", "--server"]
