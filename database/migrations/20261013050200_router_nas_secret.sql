-- RWISP Phase 2: per-router NAS RADIUS shared secret (AES-256-GCM encrypted),
-- used to keep the FreeRADIUS `nas` table in sync via `read_clients = yes`.
-- Never returned by the API or written to logs.

ALTER TABLE `tbl_routers`
    ADD COLUMN `nas_secret_encrypted` VARBINARY(255) NULL;
