//! Standards and representation boundary for Glaux Server.
//!
//! Dependencies point inward to the domain package. Structural validation uses
//! pinned original schemas; codecs and resource semantics remain later work.

pub mod projection;
pub mod scalar;
mod schema_guard;
pub mod validation;
