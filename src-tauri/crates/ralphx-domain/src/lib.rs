pub mod agents;
pub mod entities;
pub mod error;
pub mod execution;
pub mod ideation;
pub mod personas;
pub mod qa;
pub mod repositories;
pub mod review;

pub use error::{AppError, AppResult};

#[doc(hidden)]
pub mod domain {
    pub use crate::{agents, entities, execution, ideation, personas, qa, repositories, review};
}
