#!/bin/sh
# Makes the test PKI for HTTPS (ADR-0228, ADR-0230): a root, a valid leaf for
# test.zoraya, an expired leaf for expired.zoraya and a leaf for other.zoraya
# (served as wrong.zoraya). The outputs are committed; run this only to make
# them again (OpenSSL 3, e.g. Homebrew's). The keys are for tests and protect
# nothing (see README.md); both roots may sign only names under .zoraya
# (name constraints), so that a leaked copy of the keys cannot vouch for a real
# site where a program trusts it. The validity of the valid ones ends in 2126.
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
CN = Zoraya test root
EOF
"$OPENSSL" ecparam -name prime256v1 -genkey -noout -out root.key
"$OPENSSL" req -x509 -new -key root.key -sha256 -days 36500 -config "$work/ca.cnf" \
  -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" \
  -addext "nameConstraints=critical,permitted;DNS:.zoraya" \
  -out root.pem
# A second root that signs nothing here: a program that trusts only this
# one must refuse every leaf above.
"$OPENSSL" ecparam -name prime256v1 -genkey -noout -out other-root.key
"$OPENSSL" req -x509 -new -key other-root.key -sha256 -days 36500 -subj "/CN=Zoraya other root" \
  -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" \
  -addext "nameConstraints=critical,permitted;DNS:.zoraya" \
  -out other-root.pem
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
./make-outside.sh
ls -l
