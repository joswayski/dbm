//! Database behavior and local persistence shared by desktop hosts. No UI runtime.
pub mod cell_values;
pub mod connection_url;
pub mod demo;
pub mod error;
pub mod export;
pub mod keyring_store;
pub mod models;
mod mysql;
mod postgres;
mod redis;
pub mod session;
pub mod sql_text;
pub mod state;
pub mod storage;
