//! Schema dispatch and version constant.
//!
//! Only version 1 exists today. A future incompatible schema adds a sibling `v2` module and an
//! explicit dispatch path in the reader rather than weakening the version check.

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

pub mod v1;
