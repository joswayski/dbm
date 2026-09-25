//! Database behavior and local persistence shared by desktop hosts. No UI runtime.
pub mod error;
pub mod keyring_store;
pub mod models;
mod mysql;
mod postgres;
mod redis;
pub mod session;
pub mod state;
pub mod storage;
