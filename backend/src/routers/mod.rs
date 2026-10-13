pub mod error;
pub mod model;
pub mod provisioning;
pub mod repository;

pub use error::RouterError;
pub use model::{NewRouter, RouterRecord, RouterUpdate};
pub use provisioning::{ProvisionPlan, ProvisionStep, apply_provisioning, plan_provisioning};
pub use repository::{
    RouterStatus, connect_router, create_router, delete_router, get_router, latest_status,
    list_routers, set_coa_secret, set_nas_secret, status_history, update_router,
};
