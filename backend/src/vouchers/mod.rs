pub mod code;
pub mod error;
pub mod model;
pub mod repository;

pub use code::{generate_code, generate_unique_codes};
pub use error::VoucherError;
pub use model::{
    ActivationOutcome, BatchRequest, GeneratedBatch, RevokeOutcome, Voucher, VoucherFilter,
    VoucherStatus,
};
pub use repository::{
    activate_voucher, batch_exists, create_batch, expire_overdue, get_voucher, get_voucher_by_code,
    list_batch_codes, list_vouchers, revoke_voucher,
};
