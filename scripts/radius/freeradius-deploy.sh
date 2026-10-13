#!/usr/bin/env bash
#
# Install (or revert) the RWISP FreeRADIUS 3.0 configuration.
#
# Dry-run by default. Reversible. Does not touch site files or your sql module.
#
# Usage:
#   scripts/radius/freeradius-deploy.sh                 # dry-run plan
#   scripts/radius/freeradius-deploy.sh --apply         # backup + copy + enable rlm_rest
#   scripts/radius/freeradius-deploy.sh --revert [EPOCH]
#
set -euo pipefail

FR_ROOT="${FR_ROOT:-/etc/freeradius/3.0}"
SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../deploy/freeradius" && pwd)"
MODE="plan"
EPOCH=""

for arg in "$@"; do
	case "$arg" in
		--apply) MODE="apply" ;;
		--revert) MODE="revert" ;;
		--plan) MODE="plan" ;;
		[0-9]*) EPOCH="$arg" ;;
		*) echo "unknown argument: $arg" >&2; exit 2 ;;
	esac
done

run() {
	if [ "$MODE" = "plan" ]; then
		echo "  would: $*"
	else
		echo "  $*"
		eval "$@"
	fi
}

latest_backup() {
	ls -1dt "${FR_ROOT}".rwisp-backup.* 2>/dev/null | head -1 || true
}

apply() {
	local backup="${FR_ROOT}.rwisp-backup.$(date +%s)"
	echo "==> Backing up ${FR_ROOT} -> ${backup}"
	run "cp -a '${FR_ROOT}' '${backup}'"

	echo "==> Copying RWISP configuration files"
	run "cp '${SRC_DIR}/mods-available/rest' '${FR_ROOT}/mods-available/rest'"
	run "cp '${SRC_DIR}/mods-available/sqlcounter.disabled' '${FR_ROOT}/mods-available/sqlcounter.disabled'"
	run "cp '${SRC_DIR}/policy.d/rwisp-rest' '${FR_ROOT}/policy.d/rwisp-rest'"
	run "cp '${SRC_DIR}/dictionary/rwisp' '${FR_ROOT}/dictionary/rwisp'"
	run "mkdir -p '${FR_ROOT}/mods-config/sql/counter/mysql'"
	run "cp '${SRC_DIR}/mods-config/sql/counter/mysql/rwisp_data_limit.conf' '${FR_ROOT}/mods-config/sql/counter/mysql/'"
	run "cp '${SRC_DIR}/mods-config/sql/counter/mysql/rwisp_time_limit.conf' '${FR_ROOT}/mods-config/sql/counter/mysql/'"

	echo "==> Enabling rlm_rest (NOT sqlcounter)"
	run "ln -sf ../mods-available/rest '${FR_ROOT}/mods-enabled/rest'"

	cat <<EOF

==> Manual edits required (not automated):
  1. In ${FR_ROOT}/dictionary  add:  \$INCLUDE dictionary/rwisp
  2. In ${FR_ROOT}/policy.d/rwisp-rest  set the bearer token:
       &REST-HTTP-Header := "Authorization: Bearer <RADIUS_INTERNAL_TOKEN>"
  3. In your HotSpot site (sites-enabled/default):
       authorize    {  ... pap   rwisp-rest }          # rwisp-rest LAST
       authenticate {  Auth-Type rest { rwisp-rest } }
       accounting   {  sql ...   rwisp-rest }
  4. Ensure the sql module keeps:  read_clients = yes ; client_table = "nas"

==> Validate:  sudo freeradius -XC
EOF
	echo "Backup retained at: ${backup} (use --revert ${backup##*.} to undo)"
}

revert() {
	local backup
	if [ -n "$EPOCH" ]; then
		backup="${FR_ROOT}.rwisp-backup.${EPOCH}"
	else
		backup="$(latest_backup)"
	fi
	if [ -z "$backup" ] || [ ! -d "$backup" ]; then
		echo "no backup found (looked for ${FR_ROOT}.rwisp-backup.*)" >&2
		exit 1
	fi
	echo "==> Reverting from ${backup}"
	run "rm -f '${FR_ROOT}/mods-enabled/rest'"
	run "rm -f '${FR_ROOT}/mods-available/rest'"
	run "rm -f '${FR_ROOT}/mods-available/sqlcounter.disabled'"
	run "rm -f '${FR_ROOT}/policy.d/rwisp-rest'"
	run "rm -f '${FR_ROOT}/dictionary/rwisp'"
	run "rm -f '${FR_ROOT}/mods-config/sql/counter/mysql/rwisp_data_limit.conf'"
	run "rm -f '${FR_ROOT}/mods-config/sql/counter/mysql/rwisp_time_limit.conf'"
	run "cp -a '${backup}/.' '${FR_ROOT}/'"
	echo "==> Validate:  sudo freeradius -XC"
}

case "$MODE" in
	apply) apply ;;
	revert) revert ;;
	*) echo "Plan (no changes). Re-run with --apply to execute."; apply ;;
esac
