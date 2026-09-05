#!/bin/bash
# Builds the whole PKI for the OpenVPN corpus generation, using the real
# easy-rsa 3 shipped in the ulpf-openvpn:local image (never on the host).
# Produces:
#   pki-out/server/{ca.crt,server.crt,server.key,dh.pem,ta.key,crl.pem}
#   pki-out/clients/good/{ca.crt,jdoe.crt,jdoe.key,ta.key}
#   pki-out/clients/badcert/{ca.crt,mallory.crt,mallory.key,ta.key}   -- cert signed by a DIFFERENT (rogue) CA
#   pki-out/clients/badtls/{ca.crt,jdoe.crt,jdoe.key,ta.key}          -- ta.key is the WRONG static key
#   pki-out/clients/badport/{ca.crt,olduser.crt,olduser.key,ta.key}   -- valid identity, client.conf points at a closed port
set -euo pipefail
cd "$(dirname "$0")"
IMG=ulpf-openvpn:local
OUT="$(pwd)/pki-out"
rm -rf "$OUT"
mkdir -p "$OUT"

docker run --rm -v "$OUT:/out" "$IMG" bash -euc '
set -e
work() { mkdir -p "$1" && cd "$1"; }

# --- main CA + server + two client identities ---
work /tmp/main-ca
export EASYRSA_BATCH=1
/usr/share/easy-rsa/easyrsa init-pki
EASYRSA_REQ_CN="ULPF Test CA" /usr/share/easy-rsa/easyrsa build-ca nopass
/usr/share/easy-rsa/easyrsa build-server-full server nopass
/usr/share/easy-rsa/easyrsa build-client-full jdoe nopass
/usr/share/easy-rsa/easyrsa build-client-full olduser nopass
/usr/share/easy-rsa/easyrsa gen-dh
/usr/share/easy-rsa/easyrsa gen-crl
openvpn --genkey secret /tmp/main-ca/ta.key
openvpn --genkey secret /tmp/main-ca/ta-wrong.key   # deliberately mismatched tls-auth key

# --- rogue CA, used only to mint a certificate the server will never trust ---
work /tmp/rogue-ca
/usr/share/easy-rsa/easyrsa init-pki
EASYRSA_REQ_CN="Rogue Test CA" /usr/share/easy-rsa/easyrsa build-ca nopass
/usr/share/easy-rsa/easyrsa build-client-full mallory nopass

# --- assemble the exact files each container needs ---
mkdir -p /out/server /out/clients/good /out/clients/badcert /out/clients/badtls /out/clients/badport
CA=/tmp/main-ca/pki
cp "$CA/ca.crt" "$CA/issued/server.crt" "$CA/private/server.key" "$CA/dh.pem" "$CA/crl.pem" /tmp/main-ca/ta.key /out/server/
cp /tmp/main-ca/ta.key /out/server/ta.key

cp "$CA/ca.crt" "$CA/issued/jdoe.crt" "$CA/private/jdoe.key" /tmp/main-ca/ta.key /out/clients/good/
cp "$CA/ca.crt" /tmp/rogue-ca/pki/issued/mallory.crt /tmp/rogue-ca/pki/private/mallory.key /tmp/main-ca/ta.key /out/clients/badcert/
cp "$CA/ca.crt" "$CA/issued/jdoe.crt" "$CA/private/jdoe.key" /tmp/main-ca/ta-wrong.key /out/clients/badtls/
mv /out/clients/badtls/ta-wrong.key /out/clients/badtls/ta.key
cp "$CA/ca.crt" "$CA/issued/olduser.crt" "$CA/private/olduser.key" /tmp/main-ca/ta.key /out/clients/badport/
chmod -R a+r /out
'
echo "PKI written to $OUT"
