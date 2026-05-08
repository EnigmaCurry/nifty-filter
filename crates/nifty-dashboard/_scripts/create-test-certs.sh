#!/usr/bin/env bash
set -euo pipefail

# ---- Configurable inputs (override via env) ----
CA_CN="${CA_CN:-Example Test CA}"
SERVER_CN="${SERVER_CN:-localhost}"

CA_DAYS="${CA_DAYS:-3650}"
SERVER_DAYS="${SERVER_DAYS:-825}"

SAN_DNS="${SAN_DNS:-$SERVER_CN}"
SAN_IP="${SAN_IP:-127.0.0.1}"

# ---- Output temp dir ----
OUTDIR="/tmp/axum-dev-test-certs"
rm -rf "${OUTDIR}"
mkdir "${OUTDIR}"
chmod 700 "$OUTDIR"
echo "Writing certs to: $OUTDIR" >&2

# ---- File paths ----
CA_KEY="$OUTDIR/ca.key.pem"
CA_CERT="$OUTDIR/ca.cert.pem"

SERVER_KEY="$OUTDIR/server.key.pem"
SERVER_CSR="$OUTDIR/server.csr.pem"
SERVER_CERT="$OUTDIR/server.cert.pem"
FULLCHAIN="$OUTDIR/server.fullchain.pem"

OPENSSL_CNF="$OUTDIR/openssl.cnf"

# ---- Minimal OpenSSL config with v3 extensions + SAN ----
{
  echo "[ req ]"
  echo "default_bits = 2048"
  echo "prompt = no"
  echo "default_md = sha256"
  echo "distinguished_name = dn"
  echo "x509_extensions = v3_ca"
  echo
  echo "[ dn ]"
  echo "CN = ${CA_CN}"
  echo
  echo "[ v3_ca ]"
  echo "subjectKeyIdentifier = hash"
  echo "authorityKeyIdentifier = keyid:always,issuer"
  echo "basicConstraints = critical, CA:true"
  echo "keyUsage = critical, keyCertSign, cRLSign"
  echo
  echo "[ server_req ]"
  echo "default_bits = 2048"
  echo "prompt = no"
  echo "default_md = sha256"
  echo "distinguished_name = server_dn"
  echo "req_extensions = v3_req"
  echo
  echo "[ server_dn ]"
  echo "CN = ${SERVER_CN}"
  echo
  echo "[ v3_req ]"
  echo "basicConstraints = CA:FALSE"
  echo "keyUsage = critical, digitalSignature, keyEncipherment"
  echo "extendedKeyUsage = serverAuth"
  echo "subjectAltName = @alt_names"
  echo
  echo "[ alt_names ]"

  i=1
  IFS=',' read -ra dns_arr <<< "$SAN_DNS"
  for d in "${dns_arr[@]}"; do
    d="$(echo "$d" | xargs)"  # trim
    [[ -n "$d" ]] && echo "DNS.${i} = ${d}" && i=$((i+1))
  done

  j=1
  IFS=',' read -ra ip_arr <<< "$SAN_IP"
  for ip in "${ip_arr[@]}"; do
    ip="$(echo "$ip" | xargs)"
    [[ -n "$ip" ]] && echo "IP.${j} = ${ip}" && j=$((j+1))
  done
} > "$OPENSSL_CNF"

# ---- 1) Create CA key + self-signed CA cert ----
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$CA_KEY"
chmod 600 "$CA_KEY"

openssl req -new -x509 \
  -key "$CA_KEY" \
  -days "$CA_DAYS" \
  -config "$OPENSSL_CNF" \
  -extensions v3_ca \
  -out "$CA_CERT"

# ---- 2) Create server key + CSR ----
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$SERVER_KEY"
chmod 600 "$SERVER_KEY"

openssl req -new \
  -key "$SERVER_KEY" \
  -out "$SERVER_CSR" \
  -config "$OPENSSL_CNF" \
  -reqexts v3_req \
  -section server_req

# ---- 3) Sign server cert with CA ----
openssl x509 -req \
  -in "$SERVER_CSR" \
  -CA "$CA_CERT" \
  -CAkey "$CA_KEY" \
  -CAcreateserial \
  -days "$SERVER_DAYS" \
  -sha256 \
  -extfile "$OPENSSL_CNF" \
  -extensions v3_req \
  -out "$SERVER_CERT"

# ---- 4) Full chain (server cert + CA cert) ----
cat "$SERVER_CERT" "$CA_CERT" > "$FULLCHAIN"
chmod 644 "$FULLCHAIN"

echo
echo "CA cert:      $CA_CERT"
echo "Server key:   $SERVER_KEY"
echo "Server cert:  $SERVER_CERT"
echo "Full chain:   $FULLCHAIN"
echo
openssl verify -CAfile "$CA_CERT" "$SERVER_CERT"
openssl x509 -in "$SERVER_CERT" -noout -subject -issuer -dates
openssl x509 -in "$SERVER_CERT" -noout -ext subjectAltName
# debug:
echo
echo "## Set the following vars in your shell:"
echo 'export AXUM_DEV_TLS_MODE=manual'
printf 'export AXUM_DEV_TLS_CERT_PATH=%q\n' "$FULLCHAIN"
printf 'export AXUM_DEV_TLS_KEY_PATH=%q\n'  "$SERVER_KEY"
