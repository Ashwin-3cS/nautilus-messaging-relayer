# Nautilus Messaging Relayer

Nitro-enclave-ready template for running the Sui Stack Messaging relayer under Nautilus.

This template is wired directly into `nautilus-ops`, so the CLI can scaffold it, generate its CI workflow, and manage the enclave lifecycle end to end.

It adapts the relayer to:
- expose enclave attestation at `/get_attestation`
- expose enclave health at `/health`
- expose relayer health at `/health_check`
- sign delivery responses from `POST /messages`
- stream logs through `/logs`
- sync group membership from Sui inside the enclave
- archive encrypted message batches to Walrus in the background

## Upstream Projects

- Sui Stack Messaging: `https://github.com/MystenLabs/sui-stack-messaging/`
- Sui Groups: `https://github.com/MystenLabs/sui-groups`
- Nautilus Ops: `https://github.com/Ashwin-3cS/nautilus-ops/`

## Architecture

- The enclave runs the relayer on TCP `3000`.
- `run.sh` exposes it on VSOCK `4000` and receives config on VSOCK `7000`.
- The parent EC2 host bridges public TCP `4000` to enclave VSOCK `4000`.
- The parent also exposes outbound VSOCK proxies for:
  - Sui RPC on `8101`
  - Walrus publisher on `8102`
  - Walrus aggregator on `8103`
- Inside the enclave, `run.sh` binds loopback shims for those remote endpoints so the relayer can use normal HTTPS URLs.

## Required GitHub Secrets

- `TEE_EC2_HOST`
- `TEE_EC2_USER`
- `TEE_EC2_SSH_KEY`
- `RELAYER_SUI_RPC_URL`
- `RELAYER_GROUPS_PACKAGE_ID`
- `RELAYER_WALRUS_PUBLISHER_URL`
- `RELAYER_WALRUS_AGGREGATOR_URL`

## Optional GitHub Secrets

- `RELAYER_WALRUS_SYNC_INTERVAL_SECS`
- `RELAYER_WALRUS_SYNC_MESSAGE_THRESHOLD`

If the optional Walrus sync secrets are unset, the relayer falls back to:
- `WALRUS_SYNC_INTERVAL_SECS=3600`
- `WALRUS_SYNC_MESSAGE_THRESHOLD=50`

For live Walrus verification, use:

```text
RELAYER_WALRUS_SYNC_INTERVAL_SECS=30
RELAYER_WALRUS_SYNC_MESSAGE_THRESHOLD=1
```

## Deploy

Generate the workflow from `nautilus-ops`:

```bash
nautilus init-ci --template messaging-relayer --cpu-count 2 --memory-mib 4096 -f Containerfile
```

Push to `main` after adding the required secrets.

## Runtime Endpoints

- `GET /health`
- `GET /get_attestation`
- `GET /health_check`
- `GET /logs?lines=N`
- `POST /messages`
- `GET /messages`
- `PUT /messages`
- `DELETE /messages/:message_id`

## CLI Checks

Read-only checks:

```bash
nautilus --template messaging-relayer status --host <EC2_IP>
nautilus --template messaging-relayer attest --host <EC2_IP> --out pcrs-live.json
nautilus --template messaging-relayer logs --host <EC2_IP> -n 100
```

On-chain registration flow:

```bash
nautilus --template messaging-relayer update-pcrs --pcr-file pcrs-live.json
nautilus --template messaging-relayer register-enclave --host <EC2_IP>
```

`verify-signature` is not a generic fit for this template. The relayer signs authenticated delivery responses from `POST /messages`, so verification should be done through the messaging E2E flow or a relayer-specific verification command.

## E2E Test

From [`scripts`](/mnt/d/projects/nautilus-messaging-relayer/scripts):

```bash
cd /mnt/d/projects/nautilus-messaging-relayer/scripts
npm install
RELAYER_URL=http://<EC2_IP>:4000 \
TEST_WALLET_PRIVATE_KEY='suiprivkey1...' \
npx tsx test-e2e.ts | tee e2e.log
```

The script uses the current published testnet Groups package ID by default. Override `GROUPS_PACKAGE_ID` only if you are targeting a different deployment.

## Walrus Verification

After setting the fast-sync secrets and redeploying:

1. Run the E2E send path once.
2. Inspect relayer logs:

```bash
nautilus --template messaging-relayer logs --host <EC2_IP> -n 300
```

Expected lines:

- `Starting WalrusSyncService`
- `Syncing 1 pending messages to Walrus`
- `Quilt stored on Walrus`
- `Walrus pending sync cycle completed successfully`

If those lines do not appear, confirm the deployed GitHub Actions secrets include the optional Walrus sync overrides and redeploy once more.
