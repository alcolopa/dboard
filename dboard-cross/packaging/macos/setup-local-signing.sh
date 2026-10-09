#!/usr/bin/env bash
# Create one persistent local signing identity. No Apple developer account required.
set -euo pipefail
IDENTITY="dboard Local Development"
KEYCHAIN="$(security default-keychain -d user | sed 's/^[[:space:]]*"//; s/"[[:space:]]*$//')"
if security find-identity -p codesigning "$KEYCHAIN" | grep -Fq "\"$IDENTITY\""; then
  echo "Signing identity already exists: $IDENTITY"
  exit 0
fi
TASK_TMP="$(mktemp -d)"
trap 'rm -rf "$TASK_TMP"' EXIT
umask 077
cat > "$TASK_TMP/cert.conf" <<'CONF'
[req]
distinguished_name = dn
x509_extensions = signing
prompt = no
[dn]
CN = dboard Local Development
[signing]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid
CONF
openssl req -new -newkey rsa:2048 -nodes -x509 -days 3650 \
  -config "$TASK_TMP/cert.conf" -keyout "$TASK_TMP/key.pem" -out "$TASK_TMP/cert.pem" 2>/dev/null
# A temporary random export password; private key is retained only in Keychain.
openssl rand -hex 32 > "$TASK_TMP/export-password"
openssl pkcs12 -export -legacy -inkey "$TASK_TMP/key.pem" -in "$TASK_TMP/cert.pem" \
  -name "$IDENTITY" -out "$TASK_TMP/identity.p12" -passout "file:$TASK_TMP/export-password"
security import "$TASK_TMP/identity.p12" -k "$KEYCHAIN" \
  -P "$(cat "$TASK_TMP/export-password")" -T /usr/bin/codesign
security find-identity -p codesigning "$KEYCHAIN"
echo "Created $IDENTITY. Use packaging/macos/run-local.sh for subsequent builds."
