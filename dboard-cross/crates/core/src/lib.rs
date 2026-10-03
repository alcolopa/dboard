//! dboard-core: database-agnostic model, safety guards, SQL generation, persistence and
//! the PostgreSQL / MySQL / MongoDB drivers. No UI code lives here so it can be tested headless.

pub mod admin;
pub mod config;
pub mod driver;
pub mod edit;
pub mod error;
pub mod model;
pub mod mongo;
pub mod mysql;
pub mod pg;
pub mod split;
pub mod safety;
pub mod schemadiff;
pub mod sql;
pub mod tunnel;
pub mod url;
pub mod dump;
mod tls;

pub use driver::Conn;
pub use error::{Error, Result};
