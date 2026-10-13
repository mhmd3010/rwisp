# RWISP FreeRADIUS 3.0.21 deployment (exact, reversible)

RWISP is the authoritative source for authentication and for
**subscription-scoped 64-bit usage/data limits** (computed over
`tbl_radius_sessions`). FreeRADIUS performs authentication against RWISP over
loopback HTTP and enforces the returned standard attributes; it is **not**
configured to enforce cumulative limits (that would be inaccurate — see
Limitations).

## Authentication ordering (why auth cannot be bypassed)

```
authorize:
   ... chap / mschap / digest / eap / files / sql / expiration / logintime / pap   (stock)
   rwisp-rest        # LAST - overwrites Auth-Type with `rest`, never `Accept`
authenticate:
   Auth-Type rest { rwisp-rest }   # verifies credential, THEN activates
accounting:
   sql               # radacct (SimUse + native accounting)
   rwisp-rest        # RWISP idempotent 64-bit session tracking
```

- The authorize endpoint is policy-only: it returns limits + `Auth-Type := rest`.
  It performs no password check and no activation.
- The stock `authorize` ends with `pap`, which sets `Auth-Type := PAP`. Placing
  `rwisp-rest` **last** ensures `Auth-Type := rest` wins. Put it earlier and
  `pap`/`chap` would take over.
- First-login activation happens only in the `authenticate` section, after the
  credential is verified, so a rejected login never activates an account/voucher.

## Prerequisites

- FreeRADIUS 3.0.21 with `rlm_rest.so` (present on this host).
- RWISP running on `127.0.0.1:3100` with `RADIUS_INTERNAL_TOKEN` set.
- A shared secret for the NAS (and for the local PAP test client).

## Apply (reversible)

Run the helper (dry-run by default):

```
scripts/radius/freeradius-deploy.sh            # prints the plan
scripts/radius/freeradius-deploy.sh --apply    # copies files + enables rlm_rest
```

The script:
1. Backs up `/etc/freeradius/3.0` to `/etc/freeradius/3.0.rwisp-backup.<epoch>`.
2. Copies `mods-available/rest`, `policy.d/rwisp-rest`, `dictionary/rwisp`, and
   `mods-available/sqlcounter.disabled` (reference only).
3. Enables `rest` with a symlink. It does **not** touch your site files.
4. Does **not** enable any sqlcounter and does **not** overwrite your `sql` module.

Then make these edits by hand (the script prints them):

1. `dictionary`: add `$INCLUDE dictionary/rwisp` (required because authorize
   returns the vendor attribute `RWISP-Subscription-Start`).
2. `policy.d/rwisp-rest`: replace `RADIUS_INTERNAL_TOKEN_PLACEHOLDER` with the
   RWISP `RADIUS_INTERNAL_TOKEN`.
3. `sites-enabled/default` (or your HotSpot site):
   - `authorize { ... pap \n rwisp-rest }`   (rwisp-rest LAST)
   - `authenticate { Auth-Type rest { rwisp-rest } }`
   - `accounting { sql ... rwisp-rest }`
4. Keep `read_clients = yes` and `client_table = "nas"` in the `sql` module so
   NAS clients stay registered in MariaDB (`nas` table). Do not disable it.

## Validate

```
sudo freeradius -XC
```

Expect `Configuration appears to be OK.` Fix any reported undefined variable or
attribute before starting the service. (`${..tls}`-style references were the
previous failure and are removed.)

## PAP authentication tests

Local PAP through FreeRADIUS (needs a client entry for 127.0.0.1 — either a
`nas` row with `nasname=127.0.0.1` + secret, or a `clients.conf` localhost
client):

```
radtest <username> '<password>' 127.0.0.1 0 <secret>
# Expect: Access-Accept, with Mikrotik-Rate-Limit / Session-Timeout / Simultaneous-Use
radtest <username> 'wrong-password' 127.0.0.1 0 <secret>
# Expect: Access-Reject
radtest <voucher-code> '<voucher-code>' 127.0.0.1 0 <secret>
# Expect: Access-Accept (voucher PAP)
```

Direct endpoint checks (bypassing FreeRADIUS, for triage):

```
curl -s -H "Authorization: Bearer $RADIUS_INTERNAL_TOKEN" -H 'Content-Type: application/json' \
  -d '{"User-Name":"<username>"}' http://127.0.0.1:3100/internal/radius/authorize
curl -s -H "Authorization: Bearer $RADIUS_INTERNAL_TOKEN" -H 'Content-Type: application/json' \
  -d '{"User-Name":"<username>","User-Password":"<password>"}' http://127.0.0.1:3100/internal/radius/authenticate
```

## Revert

```
scripts/radius/freeradius-deploy.sh --revert [<backup-epoch>]
```

This removes the `rest` symlink and the copied files, and restores
`/etc/freeradius/3.0` from the backup. If you edited site files by hand, restore
them from the same backup directory. Then `sudo freeradius -XC` and restart.

## Design decisions / limitations (report, not worked around)

- **No FreeRADIUS sqlcounter by default.** `radacct` here has no
  `acctinputgigawords`/`acctoutputgigawords`, so radacct-based counters cannot
  represent >4 GiB and would under-count. RWISP enforces subscription-scoped
  64-bit limits authoritatively. `mods-available/sqlcounter.disabled` +
  `mods-config/sql/counter/mysql/rwisp_*.conf` are corrected reference only;
  enabling requires adding the gigaword columns (FreeRADIUS schema change).
- **Vendor attribute dependency.** authorize returns
  `reply:RWISP-Subscription-Start`; `dictionary/rwisp` must be included or
  `rlm_rest` will fail to set an unknown attribute (fail closed). If you prefer
  zero custom attributes, drop that attribute from the RWISP response and the
  dictionary include.
- **CHAP** is verifiable only when `CHAP-Challenge` is regenerated and
  forwarded; `rlm_rest` does not expose the request authenticator. Accounts are
  PAP-only by design (Argon2id). Without a challenge, CHAP fails closed.
- **CoA/Disconnect** is not sent by RWISP; use `scripts/radius/disconnect.sh`
  (radclient). NAT/CGNAT routers without a reachable disconnect endpoint require
  the RouterOS management API (`tbl_routers.disconnect_reachable = 0`).
- The configuration is validated syntactically (`freeradius -XC`) but has **not**
  been exercised against a live NAS here.
