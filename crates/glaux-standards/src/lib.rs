//! Standards and representation boundary for Glaux Server.
//!
//! Dependencies point inward to the domain package. Structural validation uses
//! pinned original schemas; codecs and resource semantics remain later work.

pub mod aggregate;
pub mod array;
pub mod choice;
pub mod geometry;
pub mod projection;
pub mod range;
pub mod scalar;
mod schema_guard;
pub mod units;
pub mod validation;
