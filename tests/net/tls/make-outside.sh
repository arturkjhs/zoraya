#!/bin/sh
# Makes outside.pem/outside.key: a leaf for www.example.org, signed by the
# test root, which may sign only names under .zoraya (nameConstraints). The
# root does not stop OpenSSL from signing it; the verifier must refuse it
# (crates/zoraya-tls/tests/handshake.rs). Uses the committed root.key and
# root.pem, so it changes nothing else; make.sh runs it at its end. OpenSSL 3.
set -eu
OPENSSL="${OPENSSL:-/opt/homebrew/bin/openssl}"
cd "$(dirname "$0")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir "$work/newcerts"
: > "$work/index.txt"
echo 2000 > "$work/serial"
cat > "$work/ca.cnf" <<CNF
[ca]
default_ca = test
[test]
dir = $work
database = \$dir/index.txt
new_certs_dir = \$dir/newcerts
serial = \$dir/serial
default_md = sha256
policy = anything
unique_subject = no
copy_extensions = copy
[anything]
commonName = supplied
CNF
"$OPENSSL" ecparam -name prime256v1 -genkey -noout -out outside.key
"$OPENSSL" req -new -key outside.key -subj "/CN=www.example.org" \
  -addext "subjectAltName=DNS:www.example.org" -addext "basicConstraints=CA:FALSE" \
  -addext "extendedKeyUsage=serverAuth" -out "$work/outside.csr"
"$OPENSSL" ca -batch -config "$work/ca.cnf" -cert root.pem -keyfile root.key \
  -startdate 20200101000000Z -enddate 21260101000000Z -in "$work/outside.csr" \
  -out outside.pem -notext
