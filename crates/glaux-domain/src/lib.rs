//! Domain boundary for Glaux Server.
//!
//! Identity, exact numeric and instant primitives are separate from resource-family
//! models, authorization, HTTP handling, database access and broker behavior.

pub mod identity;
pub mod numeric;
pub mod temporal;
