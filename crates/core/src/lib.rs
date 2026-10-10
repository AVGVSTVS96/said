pub mod bindings;
pub mod fake;
pub mod history;
pub mod session;
pub mod types;

pub use history::History;
pub use session::Session;
pub use types::*;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub const SCHEMA_VERSION: u32 = 1;
pub const SCHEMA: &str = include_str!("../migrations/001.sql");
