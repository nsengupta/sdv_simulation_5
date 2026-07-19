//! Schema dispatch and version constant.
//!
//! Archival DTOs live in [`v1`] (module name retained). [`CURRENT_SCHEMA_VERSION`] is **2**
//! after structured diagnostic `kind`. A future incompatible shape adds a sibling module and
//! an explicit reader dispatch path rather than weakening the version check.

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

pub mod v1;
