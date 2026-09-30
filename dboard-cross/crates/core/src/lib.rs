//! dboard-core: database-agnostic model, safety guards, SQL generation and
//! the PostgreSQL driver. No UI code lives here so it can be tested headless.

pub mod config;
pub mod edit;
pub mod error;
pub mod model;
pub mod postgres;
pub mod safety;
pub mod sql;

pub use error::{Error, Result};
