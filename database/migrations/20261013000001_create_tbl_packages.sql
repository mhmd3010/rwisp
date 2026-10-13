-- RWISP: HotSpot package catalogue and router-specific assignments.
-- FreeRADIUS tables are left untouched. A package with no rows in
-- tbl_router_packages is treated as global (available to every router).

CREATE TABLE IF NOT EXISTS `tbl_packages` (
    `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    `name` VARCHAR(100) NOT NULL,
    `description` VARCHAR(255) NULL,
    `price_cents` BIGINT UNSIGNED NOT NULL DEFAULT 0,
    `enabled` TINYINT(1) NOT NULL DEFAULT 1,
    `validity_secs` INT UNSIGNED NULL,
    `usage_limit_secs` INT UNSIGNED NULL,
    `data_limit_bytes` BIGINT UNSIGNED NULL,
    `upload_rate_bps` BIGINT UNSIGNED NULL,
    `download_rate_bps` BIGINT UNSIGNED NULL,
    `device_limit` INT UNSIGNED NULL,
    `created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    PRIMARY KEY (`id`),
    UNIQUE KEY `uq_tbl_packages_name` (`name`),
    KEY `idx_tbl_packages_enabled` (`enabled`)
) ENGINE = InnoDB
  DEFAULT CHARSET = utf8mb4
  COLLATE = utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_router_packages` (
    `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    `router_id` BIGINT UNSIGNED NOT NULL,
    `package_id` BIGINT UNSIGNED NOT NULL,
    `created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (`id`),
    UNIQUE KEY `uq_tbl_router_packages` (`router_id`, `package_id`),
    KEY `idx_tbl_router_packages_package` (`package_id`),
    CONSTRAINT `fk_tbl_router_packages_router`
        FOREIGN KEY (`router_id`) REFERENCES `tbl_routers` (`id`) ON DELETE CASCADE,
    CONSTRAINT `fk_tbl_router_packages_package`
        FOREIGN KEY (`package_id`) REFERENCES `tbl_packages` (`id`) ON DELETE CASCADE
) ENGINE = InnoDB
  DEFAULT CHARSET = utf8mb4
  COLLATE = utf8mb4_unicode_ci;
