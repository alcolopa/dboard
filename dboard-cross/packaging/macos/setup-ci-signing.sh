#!/usr/bin/env bash
# One-time: create a persistent self-signed code-signing certificate for release builds and store it in
# this repository's GitHub secrets (MACOS_CERT_P12, MACOS_CERT_PASSWORD, MACOS_SIGN_IDENTITY).
# Requires an authenticated `gh` and admin rights on the repository. Nothing secret is printed or kept on disk
# after the script exits. Back up the secrets yourself: losing the key means a new identity (and one more
# Keychain prompt for users). Never regenerate it for routine releases.
set -euo pipefail
REPO="${1:-$(gh repo view --json nameWithOwner -q .nameWithOwner)}"
IDENTITY="dboard Release"
for s in MACOS_CERT_P12 MACOS_SIGN_IDENTITY; do
  if gh secret list --repo "$REPO" | grep -q "^$s"; then
    echo "Secret $s already exists in $REPO; refusing to replace the release identity." >&2
    exit 1
  fi
done
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
umask 077
cat > "$TMP/cert.conf" <<CONF
[req]
distinguished_name = dn
x509_extensions = signing
prompt = no
[dn]
CN = $IDENTITY
[signing]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
subjectKeyIdentifier = hash
CONF
openssl req -new -newkey rsa:2048 -nodes -x509 -days 36500 -config "$TMP/cert.conf" \
  -keyout "$TMP/key.pem" -out "$TMP/cert.pem" 2>/dev/null
PASS="$(openssl rand -hex 32)"
openssl pkcs12 -export -legacy -inkey "$TMP/key.pem" -in "$TMP/cert.pem" -name "$IDENTITY" \
  -out "$TMP/identity.p12" -passout "pass:$PASS"
base64 < "$TMP/identity.p12" | tr -d '\n' | gh secret set MACOS_CERT_P12 --repo "$REPO"
printf '%s' "$PASS" | gh secret set MACOS_CERT_PASSWORD --repo "$REPO"
printf '%s' "$IDENTITY" | gh secret set MACOS_SIGN_IDENTITY --repo "$REPO"
echo "Release signing identity stored in $REPO secrets. The next release is signed with it."
