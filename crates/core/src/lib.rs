//! ff-core — the UI-agnostic engine for ff.
//!
//! Contract (PLAN.md §1): no rendering, no windowing, no egui. The shell must
//! never be able to block its frame thread by accident: everything that can
//! touch disk lives behind an explicit, documented API in this crate.

pub mod cfg;
pub mod find;
pub mod model;
pub mod scan;
pub mod session;
