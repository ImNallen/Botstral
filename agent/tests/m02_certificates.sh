#!/usr/bin/env bash
set -euo pipefail
umask 077

output=${1:?Provide an empty fixture output directory.}
manager_ip=${2:?Provide the manager IPv4 address.}
python3 - "$manager_ip" <<'PYIP'
import ipaddress, sys
ipaddress.IPv4Address(sys.argv[1])
PYIP
mkdir -p "$output"
test -z "$(ls -A "$output")"
root=$(cd "$output" && pwd)
certs="$root/certificates"
private="$root/private"
mkdir -p "$certs" "$private"
chmod 0700 "$certs" "$private"
test ! -e "$private/ca.key"

cat > "$private/ca.conf" <<'EOF'
[req]
distinguished_name=dn
x509_extensions=ca
[dn]
[ca]
basicConstraints=critical,CA:TRUE
keyUsage=critical,keyCertSign,cRLSign
subjectKeyIdentifier=hash
EOF

for authority in ca wrong-ca; do
    openssl genrsa -out "$private/$authority.key" 3072
    openssl req -new -x509 -sha256 -key "$private/$authority.key" \
        -subj "/CN=Botstral-M02-Acceptance-$authority" -days 7 \
        -config "$private/ca.conf" -out "$certs/$authority.pem"
done

cat > "$private/server.ext" <<EOF
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:botstral-manager,IP:$manager_ip
EOF

cat > "$private/client.ext" <<'EOF'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=clientAuth
EOF

for identity in manager worker other wrong-client expired; do
    openssl genrsa -out "$certs/$identity.key" 2048
    openssl req -new -sha256 -key "$certs/$identity.key" \
        -subj "/CN=Botstral-M02-Acceptance-$identity" -out "$private/$identity.csr"
    if test "$identity" = expired; then
        continue
    fi
    authority=ca
    extensions=client
    if test "$identity" = manager; then
        extensions=server
    elif test "$identity" = wrong-client; then
        authority=wrong-ca
    fi
    openssl x509 -req -sha256 -in "$private/$identity.csr" \
        -CA "$certs/$authority.pem" -CAkey "$private/$authority.key" \
        -CAserial "$private/$authority.serial" -CAcreateserial \
        -days 2 -extfile "$private/$extensions.ext" -out "$certs/$identity.pem"
done

: > "$private/index.txt"
printf '1000\n' > "$private/serial"
cat > "$private/sign-expired.conf" <<EOF
[ca]
default_ca=fixture
[fixture]
database=$private/index.txt
new_certs_dir=$private
serial=$private/serial
certificate=$certs/ca.pem
private_key=$private/ca.key
default_md=sha256
policy=policy
x509_extensions=client
[policy]
commonName=supplied
[client]
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=clientAuth
EOF
openssl ca -batch -config "$private/sign-expired.conf" \
    -in "$private/expired.csr" -out "$certs/expired.pem" \
    -startdate 20200101000000Z -enddate 20200102000000Z -notext

openssl verify -CAfile "$certs/ca.pem" "$certs/manager.pem" "$certs/worker.pem" "$certs/other.pem"
if openssl verify -CAfile "$certs/ca.pem" "$certs/expired.pem"; then
    exit 1
fi
if openssl verify -CAfile "$certs/ca.pem" "$certs/wrong-client.pem"; then
    exit 1
fi
find "$certs" "$private" -type f -exec chmod 0600 {} +
