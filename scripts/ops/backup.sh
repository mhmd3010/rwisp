#!/usr/bin/env bash
#
# Back up the RWISP/FreeRADIUS `radius` database and RWISP configuration.
# Dry-run by default.
#
# Usage:
#   scripts/ops/backup.sh [--apply] [output-dir]
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MODE="plan"
OUT=""
for arg in "$@"; do
	case "$arg" in
		--apply) MODE="apply" ;;
		*) OUT="$arg" ;;
	esac
done

STAMP="$(date +%Y%m%d-%H%M%S)"
OUT="${OUT:-${ROOT}/backups/${STAMP}}"

DB_HOST="${DATABASE_HOST:-localhost}"
DB_PORT="${DATABASE_PORT:-3306}"
DB_USER="${DATABASE_USER:-radius}"
DB_PASS="${DATABASE_PASSWORD:-changeme}"
DB_NAME="${DATABASE_NAME:-radius}"

echo "Backup destination: ${OUT}"
echo "  - mysqldump ${DB_NAME} (preserves all tables/rows)"
echo "  - tar of backend/ deploy/ scripts/ .env*.example (no secrets printed)"

if [ "${MODE}" = "apply" ]; then
	mkdir -p "${OUT}"
	MYSQL_PWD="${DB_PASS}" mysqldump -h"${DB_HOST}" -P"${DB_PORT}" -u"${DB_USER}" \
		--single-transaction --routines --events "${DB_NAME}" > "${OUT}/radius.sql"
	tar -czf "${OUT}/rwisp-config.tar.gz" -C "${ROOT}" \
		backend/src backend/Cargo.toml backend/Cargo.lock backend/migrations 2>/dev/null \
		|| tar -czf "${OUT}/rwisp-config.tar.gz" -C "${ROOT}" \
			backend/Cargo.toml backend/Cargo.lock deploy scripts database
	echo "Backup written to ${OUT}"
else
	echo "Dry run. Re-run with --apply to create the backup."
fi
