-- RWISP: separate account lifecycle status from package/subscription status.
-- Account status now holds only lifecycle states; subscription expiry is tracked
-- on tbl_account_packages (derived for the API).

UPDATE `tbl_accounts` SET `status` = 'active' WHERE `status` = 'expired';

ALTER TABLE `tbl_accounts`
    MODIFY COLUMN `status` ENUM('pending', 'active', 'suspended', 'disabled')
        NOT NULL DEFAULT 'pending';

ALTER TABLE `tbl_account_packages`
    MODIFY COLUMN `status` ENUM('active', 'scheduled', 'expired', 'completed', 'cancelled')
        NOT NULL DEFAULT 'active';
