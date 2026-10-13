-- RWISP Phase 2: session management, disconnect/CoA, scheduler, router status,
-- admin tokens and audit. FreeRADIUS core tables are unchanged.

CREATE TABLE IF NOT EXISTS `tbl_router_status` (
    `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    `router_id` BIGINT UNSIGNED NOT NULL,
    `checked_at` DATETIME NOT NULL,
    `reachable` TINYINT(1) NOT NULL DEFAULT 0,
    `latency_ms` INT UNSIGNED NULL,
    `version` VARCHAR(64) NULL,
    `identity` VARCHAR(128) NULL,
    `error` VARCHAR(255) NULL,
    PRIMARY KEY (`id`),
    KEY `idx_tbl_router_status_router` (`router_id`, `checked_at`)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_disconnects` (
    `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    `username` VARCHAR(100) NOT NULL,
    `account_id` BIGINT UNSIGNED NULL,
    `voucher_id` BIGINT UNSIGNED NULL,
    `session_row_id` BIGINT UNSIGNED NULL,
    `router_id` BIGINT UNSIGNED NULL,
    `nas_ip` VARCHAR(45) NULL,
    `acct_session_id` VARCHAR(64) NULL,
    `reason` VARCHAR(64) NOT NULL,
    `method` ENUM('coa', 'routeros', 'none') NOT NULL DEFAULT 'coa',
    `status` ENUM('pending', 'success', 'failed', 'unsupported') NOT NULL DEFAULT 'pending',
    `attempts` INT UNSIGNED NOT NULL DEFAULT 0,
    `max_attempts` INT UNSIGNED NOT NULL DEFAULT 5,
    `last_error` VARCHAR(255) NULL,
    `next_attempt_at` DATETIME NULL,
    `created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    `completed_at` DATETIME NULL,
    PRIMARY KEY (`id`),
    KEY `idx_tbl_disconnects_status` (`status`, `next_attempt_at`),
    KEY `idx_tbl_disconnects_session` (`nas_ip`, `acct_session_id`),
    KEY `idx_tbl_disconnects_account` (`account_id`),
    KEY `idx_tbl_disconnects_voucher` (`voucher_id`),
    KEY `idx_tbl_disconnects_router` (`router_id`)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_scheduler_state` (
    `job_name` VARCHAR(64) NOT NULL,
    `lease_owner` VARCHAR(128) NULL,
    `lease_until` DATETIME NULL,
    `last_started_at` DATETIME NULL,
    `last_finished_at` DATETIME NULL,
    `last_status` VARCHAR(32) NULL,
    `last_error` VARCHAR(255) NULL,
    `last_detail` VARCHAR(255) NULL,
    `runs` BIGINT UNSIGNED NOT NULL DEFAULT 0,
    `updated_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    PRIMARY KEY (`job_name`)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_admin_tokens` (
    `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    `name` VARCHAR(100) NOT NULL,
    `token_hash` VARCHAR(255) NOT NULL,
    `role` ENUM('admin', 'operator', 'readonly') NOT NULL DEFAULT 'readonly',
    `enabled` TINYINT(1) NOT NULL DEFAULT 1,
    `created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    `last_used_at` DATETIME NULL,
    PRIMARY KEY (`id`),
    UNIQUE KEY `uq_tbl_admin_tokens_name` (`name`)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_admin_audit` (
    `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    `actor` VARCHAR(100) NOT NULL,
    `role` VARCHAR(32) NOT NULL,
    `action` VARCHAR(64) NOT NULL,
    `target_type` VARCHAR(32) NULL,
    `target_id` VARCHAR(64) NULL,
    `detail` VARCHAR(500) NULL,
    `created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (`id`),
    KEY `idx_tbl_admin_audit_actor` (`actor`),
    KEY `idx_tbl_admin_audit_created` (`created_at`)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_unicode_ci;
