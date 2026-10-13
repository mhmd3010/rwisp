# RWISP Phase 2 — Operations & Architecture

RWISP shares the `radius` database with FreeRADIUS. RWISP-owned tables use the
`tbl_` prefix; FreeRADIUS tables (`nas`, `radcheck`, `radreply`, `radacct`, …)
are never altered.

## Components

- **API** (`rwisp-server`, Axum, `127.0.0.1:3100`)
  - `/api/*` — administrative API (bearer token: bootstrap `ADMIN_API_TOKEN`
    role=admin, plus DB tokens in `tbl_admin_tokens`, Argon2id-hashed).
  - `/internal/radius/*` — loopback-only, authenticated with `RADIUS_INTERNAL_TOKEN`;
    called by FreeRADIUS `rlm_rest` only.
- **Scheduler** (in-process tokio task; `SCHEDULER_ENABLED`). Lease-protected via
  `tbl_scheduler_state` so multiple instances never run the same job concurrently.
- **Disconnect/CoA** — CoA via `radclient` (RFC 5176, Disconnect-Request);
  RouterOS management-API fallback; NAT-aware (`tbl_routers.coa_address`,
  `disconnect_reachable`).

## Database (RWISP tables)

| Table | Purpose |
|-------|---------|
| `tbl_routers` | Router mgmt + RADIUS/NAS identity, CoA/disconnect config, encrypted secrets |
| `tbl_router_packages` | package ↔ router assignments |
| `tbl_packages` | package catalogue (speed/data/time/validity/device limits) |
| `tbl_vouchers`, `tbl_voucher_batches` | voucher lifecycle |
| `tbl_accounts`, `tbl_account_packages` | account lifecycle + renewal history |
| `tbl_radius_sessions` | idempotent 64-bit session tracking (authoritative usage) |
| `tbl_disconnects` | disconnect queue + audit (pending/success/failed/unsupported, backoff) |
| `tbl_scheduler_state` | job leases + last-run/status |
| `tbl_router_status` | router health history |
| `tbl_admin_tokens`, `tbl_admin_audit` | admin tokens (hashed) + audit log |

## RADIUS flow

1. `authorize` (policy only) → `control:Auth-Type := rest`, limits, rate limit,
   `Simultaneous-Use`, `Session-Timeout`; never `Accept`; no activation.
2. `authenticate` (`Auth-Type rest`) → verifies Argon2id (accounts, PAP) or
   voucher PAP/CHAP, then atomically activates on first login.
3. `accounting` → `tbl_radius_sessions` (idempotent, monotonic, 64-bit) and totals
   into `tbl_accounts`/`tbl_vouchers`. FreeRADIUS writes `radacct` in parallel.

RWISP Rust is **authoritative** for subscription-scoped 64-bit data/time limits.
FreeRADIUS `sqlcounter` is intentionally disabled (radacct here lacks gigaword
columns) — see `deploy/freeradius/README.md`.

## Enforcement

- Speed: `Mikrotik-Rate-Limit` reply attribute.
- Validity: reject when expired + `Session-Timeout` = remaining validity.
- Data/time quota: enforced in Rust `authorize` over `tbl_radius_sessions`,
  scoped to the current subscription window; over-limit sessions are enqueued for
  disconnect by the scheduler.
- Concurrency/device limit: `Simultaneous-Use` returned, plus Rust
  `open_count_for_username` for race-safe admission checks. Accounting delays mean
  enforcement is near-real-time, not instantaneous.

## Scheduler jobs

`expire_subscriptions`, `expire_vouchers`, `activate_renewals`,
`enqueue_disconnects`, `process_disconnects`, `detect_stale_sessions`,
`router_health`, `nas_sync`, `cleanup`. Status: `GET /api/scheduler/jobs`.

## NAS synchronization

Enabled routers with `radius_nas_ip` and a stored NAS secret are upserted into the
FreeRADIUS `nas` table by `nasname` only (never deleting operator-managed rows).
FreeRADIUS caches SQL-backed clients (`read_clients = yes`); **reload/restart
FreeRADIUS** for client changes to take effect — do not assume live pickup.

## Deployment (Debian 11)

- Unit: `deploy/systemd/rwisp-backend.service`, env `/etc/rwisp/rwisp.env`
  (root:root 0600) from `deploy/systemd/rwisp.env.example`.
- Migrations: `scripts/ops/migrate.sh --apply` (tracked in `tbl_schema_migrations`).
- Backup/restore: `scripts/ops/backup.sh --apply`, `scripts/ops/restore.sh <dir> --yes`.
- FreeRADIUS config: `deploy/freeradius/` + `scripts/radius/freeradius-deploy.sh`.
- Health: `GET /api/health` (process), `GET /api/health/db` (database).
- Graceful shutdown: SIGTERM stops accepting connections and lets the scheduler
  finish its tick (`TimeoutStopSec=30`).

## Known limitations / infrastructure

- Real end-to-end RADIUS authentication has **not** been verified; the Rust/DB
  tests pass, but a live NAS + FreeRADIUS run is required before production.
- RouterOS management fallback and provisioning are implemented against
  documented v7 API commands but untested against a live CHR.
- CoA requires a reachable disconnect endpoint and secret; NAT/CGNAT routers
  without one are marked `unsupported` (preserved, not falsely reported).
- FreeRADIUS `sqlcounter` disabled (radacct lacks gigaword columns).
- Financial reporting is not provided (no payment records).
