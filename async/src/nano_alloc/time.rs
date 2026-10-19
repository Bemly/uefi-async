//! Backward-compatible re-exports.
//!
//! Timing futures are executor-agnostic and now live in
//! [`crate::common::time`]; this module re-exports them under their
//! historical paths so existing code keeps compiling.

pub use crate::common::time::{_Timeout, _WaitTimer, Timeout, WaitTimer};
