#!/bin/bash
# Run this on the EC2 HOST to bridge VSOCK traffic to TCP.
# The enclave exposes the messaging-relayer on VSOCK port 4000.
# This script makes it available at localhost:4000 on the host.

set -e

SUI_PROXY_VSOCK_PORT=8101
WALRUS_PUBLISHER_PROXY_VSOCK_PORT=8102
WALRUS_AGGREGATOR_PROXY_VSOCK_PORT=8103

SUI_RPC_URL="${SUI_RPC_URL:-https://fullnode.testnet.sui.io:443}"
WALRUS_PUBLISHER_URL="${WALRUS_PUBLISHER_URL:-https://publisher.walrus-testnet.walrus.space}"
WALRUS_AGGREGATOR_URL="${WALRUS_AGGREGATOR_URL:-https://aggregator.walrus-testnet.walrus.space}"

extract_url_host() {
    printf '%s' "$1" | sed -E 's#^[a-zA-Z][a-zA-Z0-9+.-]*://(\[[^]]+\]|[^/:]+).*#\1#'
}

extract_url_port() {
    local url="$1"
    local explicit_port
    explicit_port=$(printf '%s' "$url" | sed -nE 's#^[a-zA-Z][a-zA-Z0-9+.-]*://[^/:]+:([0-9]+).*$#\1#p')
    if [ -n "$explicit_port" ]; then
        printf '%s' "$explicit_port"
        return
    fi

    local scheme
    scheme=$(printf '%s' "$url" | sed -nE 's#^([a-zA-Z][a-zA-Z0-9+.-]*)://.*#\1#p')
    case "$scheme" in
        https) printf '443' ;;
        http) printf '80' ;;
        *) printf '443' ;;
    esac
}

start_outbound_proxy() {
    local name="$1"
    local url="$2"
    local vsock_port="$3"
    local host
    local port

    host=$(extract_url_host "$url")
    port=$(extract_url_port "$url")

    if [ -z "$host" ] || [ -z "$port" ]; then
        echo "Skipping $name outbound proxy: could not parse URL '$url'"
        return
    fi

    echo "Forwarding enclave VSOCK:${vsock_port} -> ${host}:${port}"
    socat VSOCK-LISTEN:${vsock_port},reuseaddr,fork TCP:${host}:${port} &
}

ENCLAVE_CID=$(sudo nitro-cli describe-enclaves | jq -r '.[0].EnclaveCID')
if [ -z "$ENCLAVE_CID" ] || [ "$ENCLAVE_CID" = "null" ]; then
    echo "No running enclave found. Start one first with: make run"
    exit 1
fi

echo "Enclave CID: $ENCLAVE_CID"

# Forward relayer: host:4000 → enclave VSOCK:4000
echo "Forwarding localhost:4000 → enclave VSOCK:4000"
socat TCP-LISTEN:4000,reuseaddr,fork VSOCK-CONNECT:${ENCLAVE_CID}:4000 &

# Collect enclave logs: enclave VSOCK:5000 → enclave.log
echo "Collecting enclave logs → enclave.log"
socat VSOCK-LISTEN:5000,reuseaddr,fork OPEN:enclave.log,creat,append &

start_outbound_proxy "Sui" "$SUI_RPC_URL" "$SUI_PROXY_VSOCK_PORT"
start_outbound_proxy "Walrus publisher" "$WALRUS_PUBLISHER_URL" "$WALRUS_PUBLISHER_PROXY_VSOCK_PORT"
start_outbound_proxy "Walrus aggregator" "$WALRUS_AGGREGATOR_URL" "$WALRUS_AGGREGATOR_PROXY_VSOCK_PORT"

echo ""
echo "Forwarding active. Test with:"
echo "  curl http://localhost:4000/health"
echo "  curl http://localhost:4000/get_attestation"
echo "  curl http://localhost:4000/health_check"
echo ""
echo "Logs: tail -f enclave.log"

wait
