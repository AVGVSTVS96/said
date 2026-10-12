#!/usr/bin/env bash
set -euo pipefail
name="${SAID_SIGN_IDENTITY:-said local signing}"
if security find-identity -v -p codesigning | grep -qF "\"$name\""; then
    echo "\"$name\" is ready for code signing."
    exit 0
fi
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
if ! security find-certificate -c "$name" -p >"$work/cert.pem" 2>/dev/null; then
    # The system LibreSSL writes a PKCS#12 file that `security import` accepts; OpenSSL 3 defaults do not.
    /usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=$name" \
        -addext "basicConstraints=critical,CA:false" \
        -addext "keyUsage=critical,digitalSignature" \
        -addext "extendedKeyUsage=critical,codeSigning" \
        -keyout "$work/key.pem" -out "$work/cert.pem" 2>/dev/null
    /usr/bin/openssl pkcs12 -export -name "$name" -inkey "$work/key.pem" -in "$work/cert.pem" \
        -passout pass:said -out "$work/identity.p12"
    security import "$work/identity.p12" -P said -T /usr/bin/codesign
fi
echo "macOS will ask for your password to trust \"$name\" for code signing."
# codesign rejects an untrusted self-signed identity with "no identity found".
security add-trusted-cert -r trustRoot -p codeSign -k "$HOME/Library/Keychains/login.keychain-db" "$work/cert.pem"
echo "\"$name\" is ready. packaging/macos.sh signs with it, so rebuilt apps keep their permissions."
