-- RWISP: HotSpot username/password accounts and package assignment/renewal history.
-- FreeRADIUS tables are unchanged. Validity starts on first successful login.

CREATE TABLE IF NOT EXISTS `tbl_accounts` (
    `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    `username` VARCHAR(100) NOT NULL,
    `password_hash` VARCHAR(255) NOT NULL,
    `full_name` VARCHAR(150) NULL,
    `email` VARCHAR(190) NULL,
    `phone` VARCHAR(40) NULL,
    `address` VARCHAR(255) NULL,
    `router_id` BIGINT UNSIGNED NULL,
    `package_id` BIGINT UNSIGNED NULL,
    `status` ENUM('pending', 'active', 'suspended', 'expired', 'disabled') NOT NULL DEFAULT 'pending',
    `activated_at` DATETIME NULL,
    `expires_at` DATETIME NULL,
    `last_login_at` DATETIME NULL,
    `last_login_ip` VARCHAR(45) NULL,
    `usage_secs` INT UNSIGNED NOT NULL DEFAULT 0,
    `bytes_in` BIGINT UNSIGNED NOT NULL DEFAULT 0,
    `bytes_out` BIGINT UNSIGNED NOT NULL DEFAULT 0,
    `created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    PRIMARY KEY (`id`),
    UNIQUE KEY `uq_tbl_accounts_username` (`username`),
    KEY `idx_tbl_accounts_status` (`status`),
    KEY `idx_tbl_accounts_package` (`package_id`),
    KEY `idx_tbl_accounts_router` (`router_id`),
    CONSTRAINT `fk_tbl_accounts_package`
        FOREIGN KEY (`package_id`) REFERENCES `tbl_packages` (`id`) ON DELETE SET NULL,
    CONSTRAINT `fk_tbl_accounts_router`
        FOREIGN KEY (`router_id`) REFERENCES `tbl_routers` (`id`) ON DELETE SET NULL
) ENGINE = InnoDB
  DEFAULT CHARSET = utf8mb4
  COLLATE = utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_account_packages` (
    `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    `account_id` BIGINT UNSIGNED NOT NULL,
    `package_id` BIGINT UNSIGNED NOT NULL,
    `activation` ENUM('immediate', 'next_expiry') NOT NULL DEFAULT 'immediate',
    `status` ENUM('active', 'scheduled', 'completed', 'cancelled') NOT NULL DEFAULT 'active',
    `starts_at` DATETIME NULL,
    `expires_at` DATETIME NULL,
    `activated_at` DATETIME NULL,
    `created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (`id`),
    KEY `idx_tbl_account_packages_account` (`account_id`),
    KEY `idx_tbl_account_packages_status` (`status`),
    CONSTRAINT `fk_tbl_account_packages_account`
        FOREIGN KEY (`account_id`) REFERENCES `tbl_accounts` (`id`) ON DELETE CASCADE,
    CONSTRAINT `fk_tbl_account_packages_package`
        FOREIGN KEY (`package_id`) REFERENCES `tbl_packages` (`id`) ON DELETE RESTRICT
) ENGINE = InnoDB
  DEFAULT CHARSET = utf8mb4
  COLLATE = utf8mb4_unicode_ci;
