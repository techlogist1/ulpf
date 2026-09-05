#!/usr/bin/env python3
"""Real traffic generator for the ULPF suricata_eve corpus capture.
Runs inside the jasonish/suricata:latest container (AlmaLinux 9, python3 stdlib only).
Every request below goes out the container's real eth0 to the real internet;
suricata -i eth0 in the same netns observes it and writes eve.json.
"""
import socket
import ssl
import subprocess
import sys
import time
import urllib.request
import urllib.error

HTTP_URLS = [
    "http://example.com/",
    "http://neverssl.com/",
    "http://httpforever.com/",
    "http://info.cern.ch/",
    "http://captive.apple.com/",
    "http://detectportal.firefox.com/success.txt",
    "http://connectivitycheck.gstatic.com/generate_204",
]

HTTPS_URLS = [
    "https://example.com/",
    "https://www.wikipedia.org/",
    "https://api.github.com/",
    "https://httpbin.org/get",
    "https://www.cloudflare.com/",
    "https://1.1.1.1/",
    "https://www.mozilla.org/",
    "https://duckduckgo.com/",
    "https://www.python.org/",
    "https://get.docker.com/",
]

DNS_NAMES = [
    "example.com", "www.wikipedia.org", "api.github.com", "httpbin.org",
    "www.cloudflare.com", "1.1.1.1.nip.io", "www.mozilla.org", "duckduckgo.com",
    "www.python.org", "get.docker.com", "neverssl.com", "httpforever.com",
    "info.cern.ch", "captive.apple.com", "detectportal.firefox.com",
    "connectivitycheck.gstatic.com", "scanme.nmap.org", "secure.eicar.org",
    "testmynids.org", "www.suricata.io", "docs.suricata.io", "www.google.com",
    "www.iana.org", "www.rfc-editor.org", "cdn.jsdelivr.net",
    "this-domain-should-not-exist-ulpf-hackathon.invalid",
    "another-bogus-name-xyz123.invalid",
]

def do_http(url, timeout=5):
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "ulpf-corpus/1.0"})
        with urllib.request.urlopen(req, timeout=timeout) as r:
            r.read(2048)
        print(f"HTTP  ok  {url}")
    except Exception as e:
        print(f"HTTP  err {url} {e}")

def do_dns(name):
    try:
        infos = socket.getaddrinfo(name, None)
        print(f"DNS   ok  {name} -> {infos[0][4][0]}")
    except Exception as e:
        print(f"DNS   err {name} {e}")

def do_tls(host, port=443, timeout=5):
    try:
        ctx = ssl.create_default_context()
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
        with socket.create_connection((host, port), timeout=timeout) as sock:
            with ctx.wrap_socket(sock, server_hostname=host) as ssock:
                ssock.getpeercert()
        print(f"TLS   ok  {host}")
    except Exception as e:
        print(f"TLS   err {host} {e}")

def do_portscan(host, ports, timeout=0.5):
    for p in ports:
        try:
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            s.settimeout(timeout)
            r = s.connect_ex((host, p))
            s.close()
            print(f"SCAN  {host}:{p} -> {'open' if r == 0 else 'closed/filtered'}")
        except Exception as e:
            print(f"SCAN  err {host}:{p} {e}")

def do_eicar():
    for url in [
        "https://secure.eicar.org/eicar.com.txt",
        "https://secure.eicar.org/eicar.com",
        "http://eicar.org/download/eicar.com.txt",
    ]:
        do_http(url)

def do_nids_test():
    for url in [
        "http://testmynids.org/uid/index.html",
    ]:
        do_http(url)

def main():
    rounds = int(sys.argv[1]) if len(sys.argv) > 1 else 6
    for i in range(rounds):
        print(f"=== round {i+1}/{rounds} ===")
        for n in DNS_NAMES:
            do_dns(n)
        for u in HTTP_URLS:
            do_http(u)
        for u in HTTPS_URLS:
            do_tls(u.split("//", 1)[1].split("/", 1)[0])
            do_http(u)
        do_eicar()
        do_nids_test()
        do_portscan("scanme.nmap.org", [21, 22, 23, 25, 80, 110, 143, 443, 3389, 8080])
        time.sleep(1)
    print("done")

if __name__ == "__main__":
    main()
