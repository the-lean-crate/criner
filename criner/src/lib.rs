#![deny(unsafe_code)]

#[macro_use]
extern crate quick_error;

pub mod error;
pub(crate) mod model;
pub(crate) mod persistence;
pub(crate) mod utils;

mod engine;

pub use engine::*;

pub use prodash;
