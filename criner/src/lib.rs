#![deny(unsafe_code)]

#[macro_use]
extern crate quick_error;

#[cfg(feature = "migration")]
pub mod migration;

pub mod error;
pub(crate) mod model;
pub(crate) mod persistence;
pub(crate) mod utils;

mod engine;

pub use engine::run;

pub use prodash;
