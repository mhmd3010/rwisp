[![StandWithPalestine](https://raw.githubusercontent.com/Safouene1/support-palestine-banner/master/StandWithPalestine.svg)](https://github.com/Safouene1/support-palestine-banner/blob/master/Markdown-pages/Support.md)

# RWISP

Hotspot captive-portal management backend. **Rust + Axum + SQLx**, **FreeRADIUS 3**, **MariaDB**, **MikroTik RouterOS**.

## Features

- Accounts (Argon2id) and vouchers, packages, renewals, expiry
- FreeRADIUS auth/accounting via a localhost REST hook
- Session tracking, usage limits, disconnect/CoA
- Multi-router management (RouterOS API)
- Background scheduler for lifecycle jobs
- Bearer-authenticated admin API

## Requirements

Rust 1.99+ · MariaDB · FreeRADIUS 3.0 · Debian 11

## Setup

```sh
cd backend
cp ../.env.example .env    # fill in secrets (never commit .env)
cargo build --release
../scripts/ops/migrate.sh --apply
cargo run --release --bin rwisp-server
```

API on `http://127.0.0.1:3100`.

## Tests

```sh
cd backend
cargo test
RWISP_RUN_DB_CRUD_TESTS=1 cargo test   # DB integration tests
```

## Layout

```
backend/   Rust API, scheduler, CLI binaries
database/  SQL migrations
deploy/    systemd + FreeRADIUS config
scripts/   ops and radius helpers
docs/      architecture and operations
```

See `docs/PHASE2.md` for architecture, API, and operations.
