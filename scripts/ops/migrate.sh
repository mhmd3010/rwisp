#!/usr/bin/env bash
#
# Apply RWISP SQL migrations in order, tracking applied files in
# tbl_schema_migrations. Dry-run by default.
#
# Usage:
#   scripts/ops/migrate.sh            # show pending migrations
#   scripts/ops/migrate.sh --apply    # apply pending migrations
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MIGRATIONS="${ROOT}/database/migrations"
MODE="plan"
[ "${1:-}" = "--apply" ] && MODE="apply"

DB_HOST="${DATABASE_HOST:-localhost}"
DB_PORT="${DATABASE_PORT:-3306}"
DB_USER="${DATABASE_USER:-radius}"
DB_PASS="${DATABASE_PASSWORD:-changeme}"
DB_NAME="${DATABASE_NAME:-radius}"

mysql_run() {
	mysql -h"${DB_HOST}" -P"${DB_PORT}" -u"${DB_USER}" -p"${DB_PASS}" "${DB_NAME}" "$@"
}

mysql_run -e "CREATE TABLE IF NOT EXISTS tbl_schema_migrations (
    name VARCHAR(255) NOT NULL PRIMARY KEY,
    applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4" >/dev/null

applied="$(mysql_run -N -e "SELECT name FROM tbl_schema_migrations")"

status=0
for file in "${MIGRATIONS}"/*.sql; do
	name="$(basename "${file}")"
	if printf '%s\n' "${applied}" | grep -qx "${name}"; then
		continue
	fi
	status=1
	echo "pending: ${name}"
	if [ "${MODE}" = "apply" ]; then
		echo "  applying ${name}"
		mysql_run < "${file}"
		mysql_run -e "INSERT IGNORE INTO tbl_schema_migrations (name) VALUES ('${name}')"
		echo "  recorded ${name}"
	fi
done

if [ "${status}" = "0" ]; then
	echo "No pending migrations."
elif [ "${MODE}" != "apply" ]; then
	echo "Re-run with --apply to apply the migrations above."
fi
