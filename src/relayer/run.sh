#!/bin/sh
set -e

export LD_LIBRARY_PATH=/lib:$LD_LIBRARY_PATH

SUI_PROXY_VSOCK_PORT=8101
WALRUS_PUBLISHER_PROXY_VSOCK_PORT=8102
WALRUS_AGGREGATOR_PROXY_VSOCK_PORT=8103

DEFAULT_WALRUS_PUBLISHER_URL="https://publisher.walrus-testnet.walrus.space"
DEFAULT_WALRUS_AGGREGATOR_URL="https://aggregator.walrus-testnet.walrus.space"

extract_url_host() {
    printf '%s' "$1" | sed -E 's#^[a-zA-Z][a-zA-Z0-9+.-]*://(\[[^]]+\]|[^/:]+).*#\1#'
}

extract_url_port() {
    url="$1"
    explicit_port=$(printf '%s' "$url" | sed -nE 's#^[a-zA-Z][a-zA-Z0-9+.-]*://[^/:]+:([0-9]+).*$#\1#p')
    if [ -n "$explicit_port" ]; then
        printf '%s' "$explicit_port"
        return
    fi

    scheme=$(printf '%s' "$url" | sed -nE 's#^([a-zA-Z][a-zA-Z0-9+.-]*)://.*#\1#p')
    case "$scheme" in
        https) printf '443' ;;
        http) printf '80' ;;
        *) printf '443' ;;
    esac
}

setup_outbound_proxy() {
    name="$1"
    url="$2"
    loopback_ip="$3"
    vsock_port="$4"

    host=$(extract_url_host "$url")
    port=$(extract_url_port "$url")

    if [ -z "$host" ] || [ -z "$port" ]; then
        echo "Skipping outbound proxy for $name: could not parse URL '$url'"
        return
    fi

    busybox ip addr add "${loopback_ip}/32" dev lo 2>/dev/null || true
    echo "${loopback_ip} ${host}" >> /etc/hosts

    echo "Outbound proxy ready: ${host}:${port} -> ${loopback_ip}:${port} -> VSOCK:${vsock_port}"
    socat TCP-LISTEN:${port},bind=${loopback_ip},reuseaddr,fork VSOCK-CONNECT:3:${vsock_port} &
}

# Setup loopback networking
busybox ip addr add 127.0.0.1/32 dev lo
busybox ip link set dev lo up
echo "127.0.0.1   localhost" > /etc/hosts

# Set enclave mode
export ENCLAVE_MODE=true
echo "Enclave mode enabled"

# ── Receive config from parent via VSOCK port 7000 ──────────────────────
# Parent sends newline-separated KEY=VALUE pairs, then closes the connection.
echo "Waiting for config on VSOCK port 7000..."
CONFIG=$(socat VSOCK-LISTEN:7000,reuseaddr - 2>/dev/null)

# Export each KEY=VALUE line as an environment variable
while IFS= read -r line; do
    case "$line" in
        *=*)
            key="${line%%=*}"
            val="${line#*=}"
            export "${key}=${val}"
            echo "Config loaded: ${key}=<set>"
            ;;
    esac
done << EOF
$CONFIG
EOF

export WALRUS_PUBLISHER_URL="${WALRUS_PUBLISHER_URL:-$DEFAULT_WALRUS_PUBLISHER_URL}"
export WALRUS_AGGREGATOR_URL="${WALRUS_AGGREGATOR_URL:-$DEFAULT_WALRUS_AGGREGATOR_URL}"

setup_outbound_proxy "sui" "$SUI_RPC_URL" "127.0.0.2" "$SUI_PROXY_VSOCK_PORT"
setup_outbound_proxy "walrus-publisher" "$WALRUS_PUBLISHER_URL" "127.0.0.3" "$WALRUS_PUBLISHER_PROXY_VSOCK_PORT"
setup_outbound_proxy "walrus-aggregator" "$WALRUS_AGGREGATOR_URL" "127.0.0.4" "$WALRUS_AGGREGATOR_PROXY_VSOCK_PORT"

echo "Config received. Starting messaging-relayer..."

# Expose messaging-relayer on VSOCK port 4000 (relayer listens on TCP 3000)
socat VSOCK-LISTEN:4000,reuseaddr,fork TCP:localhost:3000 &

/messaging_relayer > /tmp/server.log 2>&1 &
SERVER_PID=$!

echo "messaging-relayer started: PID $SERVER_PID (port 4000)"

# Forward logs to host via VSOCK port 5000
(tail -f /tmp/server.log 2>/dev/null | socat - VSOCK-CONNECT:3:5000 2>/dev/null) &

wait $SERVER_PID
