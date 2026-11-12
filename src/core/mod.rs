pub mod config;
pub mod event;
pub mod model;
pub mod notify;
pub mod paths;
pub mod scope;
pub mod storage;

pub use config::Config;
pub use event::{AppEvent, EventBus};
pub use model::*;
pub use notify::Notifier;
pub use paths::{config_path, data_dir, ensure_dirs, scans_dir};
pub use scope::Scope;
pub use storage::Store;
