#!/usr/bin/env bash
# Run on Foundation with the directory produced by build-client.sh.
set -euo pipefail
stage="${1:?usage: install-foundation.sh STAGED_BUILD}"
site=/var/www/foundation
domain=subspacefoundation.com
address=5.161.238.245
port=4443
gateway=/opt/foundation-web-gateway
unit=foundation-browser-public
unit_file="/etc/systemd/system/$unit.service"

test "$(id -u)" = 0
test -f "$stage/index.html"
test -f "$stage/pkg/oss_client_bg.wasm"
test -d "$site"
test ! -e "$site/play" || { echo "A /play deployment already exists." >&2; exit 1; }
test ! -e "$unit_file" || { echo "A browser gateway service already exists." >&2; exit 1; }
ip -o -4 addr show | grep -Fw "$address" >/dev/null
"$gateway/.venv/bin/python" -c 'import aioquic'
systemctl is-active --quiet caddy
systemctl is-active --quiet subspace

# Match the TLS certificate Caddy is serving now. Pass it through systemd
# credentials so the dedicated gateway user never needs Caddy's key directory.
fingerprint="$(
  timeout 8 openssl s_client -connect 127.0.0.1:443 -servername "$domain" </dev/null 2>/dev/null |
    openssl x509 -noout -fingerprint -sha256
)"
cert=
while IFS= read -r -d '' path; do
  if [[ "$(openssl x509 -in "$path" -noout -fingerprint -sha256)" == "$fingerprint" ]] &&
    [[ -s "${path%.crt}.key" ]]; then
    cert="$path"
    break
  fi
done < <(find /var/lib/caddy -type f -name "$domain.crt" -print0)
if [[ -z "$cert" ]]; then
  echo 'Could not locate the active Foundation certificate in Caddy storage.' >&2
  exit 1
fi
openssl x509 -in "$cert" -noout -checkend 86400

work="$(mktemp -d "$site/.browser-build.XXXXXXXX")"
installed=0
started=0
published=0
cleanup() {
  result=$?
  trap - EXIT
  if (( result != 0 )); then
    if (( published )); then mv -- "$site/play" "$work/play"; fi
    if (( started )); then systemctl disable --now "$unit" || true; fi
    if (( installed )); then
      rm -- "$unit_file"
      systemctl daemon-reload
    fi
  fi
  rm -rf -- "$work"
  exit "$result"
}
trap cleanup EXIT

mkdir -m 0755 "$work/play"
cp -R "$stage/." "$work/play/"
chmod -R a+rX "$work/play"
cat > "$unit_file" <<UNIT
[Unit]
Description=Foundation browser WebTransport gateway
After=network-online.target subspace.service
Wants=network-online.target

[Service]
DynamicUser=yes
NoNewPrivileges=yes
LoadCredential=cert.pem:$cert
LoadCredential=key.pem:${cert%.crt}.key
ExecStart=$gateway/.venv/bin/python $gateway/gateway.py --bind $address --port $port --origin https://$domain --zone-ip 127.0.0.1 --zone-port 5000 --cert %d/cert.pem --key %d/key.pem
Restart=on-failure

[Install]
WantedBy=multi-user.target
UNIT
installed=1
systemctl daemon-reload
started=1
systemctl enable --now "$unit"
for attempt in {1..20}; do
  if ss -Hlun 'sport = :4443' | grep -F "$address:$port" >/dev/null; then break; fi
  sleep 0.2
done
systemctl is-active --quiet "$unit"
ss -Hlun 'sport = :4443' | grep -F "$address:$port" >/dev/null

mv -T -- "$work/play" "$site/play"
published=1
curl --max-time 8 --fail --silent --show-error --resolve "$domain:443:127.0.0.1" \
  "https://$domain/play/" | grep -F 'Launch Dare'
curl --max-time 8 --fail --silent --show-error --head --resolve "$domain:443:127.0.0.1" \
  "https://$domain/play/pkg/oss_client_bg.wasm" | grep -i '^content-type: application/wasm'
echo "Installed https://$domain/play/ with the gateway listening on UDP $port."
