-- RWISP Phase 2: per-router CoA/Disconnect shared secret (AES-256-GCM encrypted,
-- like the router API password). Never returned by the API or written to logs.

ALTER TABLE `tbl_routers`
    ADD COLUMN `coa_secret_encrypted` VARBINARY(255) NULL;
