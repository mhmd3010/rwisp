-- RWISP: registered MikroTik routers used by the rwispros module.
-- FreeRADIUS tables are intentionally left untouched; NAS/radius data is
-- independent of this table for now.

CREATE TABLE IF NOT EXISTS `tbl_routers` (
    `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    `name` VARCHAR(100) NOT NULL,
    `host` VARCHAR(255) NOT NULL,
    `api_port` SMALLINT UNSIGNED NOT NULL DEFAULT 8728,
    `username` VARCHAR(100) NOT NULL,
    `password_encrypted` VARBINARY(255) NOT NULL,
    `transport` ENUM('tcp', 'tls', 'tls_insecure') NOT NULL DEFAULT 'tcp',
    `timeout_secs` INT UNSIGNED NOT NULL DEFAULT 10,
    `radius_nas_ip` VARCHAR(45) NULL,
    `enabled` TINYINT(1) NOT NULL DEFAULT 1,
    `created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    PRIMARY KEY (`id`),
    UNIQUE KEY `uq_tbl_routers_name` (`name`),
    KEY `idx_tbl_routers_enabled` (`enabled`)
) ENGINE = InnoDB
  DEFAULT CHARSET = utf8mb4
  COLLATE = utf8mb4_unicode_ci;
