// #![deny(missing_docs, dead_code)]

//! ### Canal
//!
//! A library for communication primitives.

#[cfg(test)] extern crate crossbeam;

pub mod broadcast;
pub mod mpmc;
mod sync;
