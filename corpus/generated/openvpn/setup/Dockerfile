FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    openvpn easy-rsa openssl rsyslog iproute2 iputils-ping procps ca-certificates \
    && rm -rf /var/lib/apt/lists/*
