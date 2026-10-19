//! Backward-compatible re-exports.
//!
//! The control-flow combinators are executor-agnostic and now live in
//! [`crate::common::control`], usable from every line (`nano-alloc`,
//! `alloc`/`dynamic`, `static`). This module only re-exports them under
//! their historical paths so existing code keeps compiling.

pub mod multiple {
    pub use crate::common::control::multiple::*;
}

pub mod single {
    pub use crate::common::control::single::*;
}
