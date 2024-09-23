//! Utility crate that contains useful, common functionality.
#![warn(missing_docs)]
#![feature(let_chains)]

pub mod collision;
pub mod container;
pub mod math;
mod traits;

pub use traits::*;
