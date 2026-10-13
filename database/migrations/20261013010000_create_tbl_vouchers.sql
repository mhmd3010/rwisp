-- RWISP: HotSpot voucher lifecycle. FreeRADIUS tables are unchanged.
-- Validity is measured from the first successful login (activated_at),
-- never from generation time.

CREATE TABLE IF NOT EXISTS `tbl_voucher_batches` (
    `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    `package_id` BIGINT UNSIGNED NOT NULL,
    `router_id` BIGINT UNSIGNED NULL,
    `quantity` INT UNSIGNED NOT NULL,
    `note` VARCHAR(255) NULL,
    `created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (`id`),
    KEY `idx_tbl_voucher_batches_package` (`package_id`),
    KEY `idx_tbl_voucher_batches_router` (`router_id`),
    CONSTRAINT `fk_tbl_voucher_batches_package`
        FOREIGN KEY (`package_id`) REFERENCES `tbl_packages` (`id`) ON DELETE RESTRICT,
    CONSTRAINT `fk_tbl_voucher_batches_router`
        FOREIGN KEY (`router_id`) REFERENCES `tbl_routers` (`id`) ON DELETE SET NULL
) ENGINE = InnoDB
  DEFAULT CHARSET = utf8mb4
  COLLATE = utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_vouchers` (
    `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    `batch_id` BIGINT UNSIGNED NULL,
    `code` VARCHAR(64) NOT NULL,
    `package_id` BIGINT UNSIGNED NOT NULL,
    `router_id` BIGINT UNSIGNED NULL,
    `status` ENUM('unused', 'active', 'expired', 'revoked') NOT NULL DEFAULT 'unused',
    `created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    `activated_at` DATETIME NULL,
    `expires_at` DATETIME NULL,
    `last_used_at` DATETIME NULL,
    `usage_secs` INT UNSIGNED NOT NULL DEFAULT 0,
    `bytes_in` BIGINT UNSIGNED NOT NULL DEFAULT 0,
    `bytes_out` BIGINT UNSIGNED NOT NULL DEFAULT 0,
    `revoked_at` DATETIME NULL,
    PRIMARY KEY (`id`),
    UNIQUE KEY `uq_tbl_vouchers_code` (`code`),
    KEY `idx_tbl_vouchers_status` (`status`),
    KEY `idx_tbl_vouchers_package` (`package_id`),
    KEY `idx_tbl_vouchers_router` (`router_id`),
    KEY `idx_tbl_vouchers_batch` (`batch_id`),
    CONSTRAINT `fk_tbl_vouchers_batch`
        FOREIGN KEY (`batch_id`) REFERENCES `tbl_voucher_batches` (`id`) ON DELETE SET NULL,
    CONSTRAINT `fk_tbl_vouchers_package`
        FOREIGN KEY (`package_id`) REFERENCES `tbl_packages` (`id`) ON DELETE RESTRICT,
    CONSTRAINT `fk_tbl_vouchers_router`
        FOREIGN KEY (`router_id`) REFERENCES `tbl_routers` (`id`) ON DELETE SET NULL
) ENGINE = InnoDB
  DEFAULT CHARSET = utf8mb4
  COLLATE = utf8mb4_unicode_ci;
