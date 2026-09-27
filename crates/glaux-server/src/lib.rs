//! Application-owned persistence adapters; SQL does not enter glaux-domain.
pub mod application;
pub mod authentication;
pub mod authorization;
mod authorization_storage;
pub mod configuration;
pub mod discovery;
pub mod http_boundary;
pub mod revisions;
pub mod runtime;
pub mod storage;
pub mod system_http;
