#!/usr/bin/env bash
#
# Send an RFC 5176 Disconnect-Request to a NAS to drop a live session.
#
# Usage:
#   scripts/radius/disconnect.sh <address> <port> <secret> <acct-session-id> [username]
#
# The address/port/secret come from the FreeRADIUS `clients.conf` entry for the
# NAS (never stored in the RWISP database). For routers behind NAT/CGNAT that
# have no reachable disconnect endpoint (tbl_routers.disconnect_reachable = 0),
# a disconnect will not succeed; use the RouterOS management API instead.
#
# Requires radclient (freeradius-utils).

set -euo pipefail

address="${1:?address required}"
port="${2:-3799}"
secret="${3:?shared secret required}"
session="${4:?acct-session-id required}"
username="${5:-}"

attributes="Acct-Session-Id=${session}"
if [ -n "${username}" ]; then
	attributes="${attributes},User-Name=${username}"
fi

printf '%s\n' "${attributes}" | radclient -x "${address}:${port}" disconnect "${secret}"
