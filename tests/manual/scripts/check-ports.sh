#!/usr/bin/env bash
# Pre-flight port check for tests/manual/ipv4-ipv6-forwarding.md lab ports.
set -euo pipefail

UDP_PORTS=(15353 15354 15300 15301)
TCP_PORTS=(15199)

check_udp() {
  local port=$1
  if ss -uln 2>/dev/null | grep -qE ":${port}\b"; then
    echo "IN USE (UDP): ${port}"
    ss -ulnp 2>/dev/null | grep -E ":${port}\b" || true
    return 1
  fi
  echo "free (UDP):   ${port}"
  return 0
}

check_tcp() {
  local port=$1
  if ss -tln 2>/dev/null | grep -qE ":${port}\b"; then
    echo "IN USE (TCP): ${port}"
    ss -tlnp 2>/dev/null | grep -E ":${port}\b" || true
    return 1
  fi
  echo "free (TCP):   ${port}"
  return 0
}

echo "=== Manual lab ports (Conduit / dnsmasq) ==="
fail=0
for p in "${UDP_PORTS[@]}"; do
  check_udp "$p" || fail=1
done
for p in "${TCP_PORTS[@]}"; do
  check_tcp "$p" || fail=1
done

echo ""
echo "=== mDNS reference (not used by lab; often Chrome/Avahi) ==="
if ss -uln 2>/dev/null | grep -qE ':5353\b'; then
  echo "NOTE: UDP 5353 is in use (typical mDNS). Lab avoids 5353 on purpose."
  ss -ulnp 2>/dev/null | grep -E ':5353\b' || true
else
  echo "UDP 5353: no listener shown"
fi

echo ""
if [[ $fail -ne 0 ]]; then
  echo "Some lab ports are busy. Stop conflicting processes or change ports in tests/manual/config/."
  exit 1
fi
echo "All lab ports appear free."
