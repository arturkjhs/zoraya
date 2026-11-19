#!/bin/sh
# Makes the test PKI of git's HTTPS test (TASK-31 10h, ADR-0461): the same chain as ../tls (a root, a valid leaf for
# test.zoraya, an expired leaf for expired.zoraya and a leaf for other.zoraya, served as wrong.zoraya), with the root's
# name constraint NOT critical. mbedTLS 3.6 cannot read a critical name constraint (parse error -0x2562) and
# `git-remote-http` is built on it (README.md); OpenSSL enforces the constraint whether critical or not, so a leaked copy of
# these keys still vouches for nothing outside .zoraya where OpenSSL, rustls or any conforming verifier trusts it. The
# outputs are committed; run this only to make them again (OpenSSL 3, e.g. Homebrew's). The keys are for tests and
# protect nothing. The validity of the valid ones ends in 2126.
set -eu
OPENSSL="${OPENSSL:-/opt/homebrew/bin/openssl}"
cd "$(dirname "$0")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir "$work/newcerts"
: > "$work/index.txt"
echo 1000 > "$work/serial"
cat > "$work/ca.cnf" <<EOF
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
[req]
distinguished_name = dn
prompt = no
[dn]
CN = Zoraya git test root
EOF
"$OPENSSL" ecparam -name prime256v1 -genkey -noout -out root.key
"$OPENSSL" req -x509 -new -key root.key -sha256 -days 36500 -config "$work/ca.cnf" \
  -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" \
  -addext "nameConstraints=permitted;DNS:.zoraya" \
  -out root.pem
leaf() { # name file startdate enddate
  "$OPENSSL" ecparam -name prime256v1 -genkey -noout -out "$2.key"
  "$OPENSSL" req -new -key "$2.key" -subj "/CN=$1" \
    -addext "subjectAltName=DNS:$1" -addext "basicConstraints=CA:FALSE" \
    -addext "extendedKeyUsage=serverAuth" -out "$work/$2.csr"
  "$OPENSSL" ca -batch -config "$work/ca.cnf" -cert root.pem -keyfile root.key \
    -startdate "$3" -enddate "$4" -in "$work/$2.csr" -out "$2.pem" -notext
}
leaf test.zoraya valid 20200101000000Z 21260101000000Z
leaf expired.zoraya expired 20200101000000Z 20200102000000Z
leaf other.zoraya wrong 20200101000000Z 21260101000000Z

ls -l
