-- RWISP Phase 1: FreeRADIUS integration support.
-- Adds RWISP-owned session tracking and per-router RADIUS/CoA reachability.
-- FreeRADIUS core tables (nas, radcheck, radreply, radacct, ...) are unchanged.

CREATE TABLE IF NOT EXISTS `tbl_radius_sessions` (
    `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    `username` VARCHAR(100) NOT NULL,
    `acct_session_id` VARCHAR(64) NOT NULL,
    `acct_unique_id` VARCHAR(32) NULL,
    `nas_ip` VARCHAR(45) NOT NULL DEFAULT '',
    `nas_identifier` VARCHAR(64) NULL,
    `calling_station_id` VARCHAR(64) NULL,
    `framed_ip` VARCHAR(45) NULL,
    `account_id` BIGINT UNSIGNED NULL,
    `voucher_id` BIGINT UNSIGNED NULL,
    `status` ENUM('start', 'interim', 'stop') NOT NULL DEFAULT 'start',
    `session_time` BIGINT UNSIGNED NOT NULL DEFAULT 0,
    `input_octets` BIGINT UNSIGNED NOT NULL DEFAULT 0,
    `output_octets` BIGINT UNSIGNED NOT NULL DEFAULT 0,
    `terminate_cause` VARCHAR(64) NULL,
    `started_at` DATETIME NULL,
    `updated_at` DATETIME NULL,
    `stopped_at` DATETIME NULL,
    `created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (`id`),
    UNIQUE KEY `uq_tbl_radius_sessions_session` (`nas_ip`, `acct_session_id`),
    KEY `idx_tbl_radius_sessions_username` (`username`),
    KEY `idx_tbl_radius_sessions_status` (`status`),
    KEY `idx_tbl_radius_sessions_account` (`account_id`),
    KEY `idx_tbl_radius_sessions_voucher` (`voucher_id`)
) ENGINE = InnoDB
  DEFAULT CHARSET = utf8mb4
  COLLATE = utf8mb4_unicode_ci;

-- Per-router RADIUS identity and disconnect (CoA/Disconnect) reachability.
-- `host` remains the management/API address used by rwispros; these fields
-- distinguish authentication identity from disconnect reachability (NAT/CGNAT).
ALTER TABLE `tbl_routers`
    ADD COLUMN `nas_identifier` VARCHAR(64) NULL,
    ADD COLUMN `coa_address` VARCHAR(255) NULL,
    ADD COLUMN `coa_port` SMALLINT UNSIGNED NULL,
    ADD COLUMN `auth_reachable` TINYINT(1) NOT NULL DEFAULT 1,
    ADD COLUMN `disconnect_reachable` TINYINT(1) NOT NULL DEFAULT 1;
