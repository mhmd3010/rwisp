#!/usr/bin/env bash
#
# Restore the `radius` database from a backup produced by backup.sh.
# Requires explicit confirmation. Dry-run by default.
#
# Usage:
#   scripts/ops/restore.sh <backup-dir>            # show plan
#   scripts/ops/restore.sh <backup-dir> --yes      # restore
#
set -euo pipefail

DIR="${1:?backup directory required}"
CONFIRM="${2:-}"
DUMP="${DIR}/radius.sql"
[ -f "${DUMP}" ] || { echo "no radius.sql found in ${DIR}" >&2; exit 1; }

DB_HOST="${DATABASE_HOST:-localhost}"
DB_PORT="${DATABASE_PORT:-3306}"
DB_USER="${DATABASE_USER:-radius}"
DB_PASS="${DATABASE_PASSWORD:-changeme}"
DB_NAME="${DATABASE_NAME:-radius}"

echo "Restore plan:"
echo "  import ${DUMP} into ${DB_NAME} on ${DB_HOST}"
echo "  WARNING: this overwrites existing RWISP and FreeRADIUS tables."

if [ "${CONFIRM}" = "--yes" ]; then
	MYSQL_PWD="${DB_PASS}" mysql -h"${DB_HOST}" -P"${DB_PORT}" -u"${DB_USER}" "${DB_NAME}" < "${DUMP}"
	echo "Restore complete. Restart rwisp-backend and reload FreeRADIUS if NAS changed."
else
	echo "Dry run. Re-run with --yes to restore."
fi
