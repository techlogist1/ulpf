import socket
s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
s.bind(("0.0.0.0", 514))
f = open("/out/haproxy.log", "ab", buffering=0)
print("syslog udp listener on :514, writing /out/haproxy.log", flush=True)
while True:
    data, addr = s.recvfrom(65535)
    f.write(data.rstrip(b"\x00") + b"\n")
