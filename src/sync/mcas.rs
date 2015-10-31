//! A multi-word compare-and-swap implementation
//!
//! Based on paper by Keir Fraser et al.

use sync::ccas::{CondAtomicPtr};
use sync::utils::{Descriptor};
use std::sync::atomic::{AtomicPtr, Ordering, AtomicUsize};

const UNDECIDED: usize = 0;
const FAILED: usize = 1;
const SUCCEEDED: usize = 2;

struct MCASDescriptor<T> {
    state: AtomicUsize,
    changes: Vec<(AtomicPtr<T>, *mut T, *mut T)>, // (atomic, current, new) tuples
}

impl<T> Descriptor<T> for MCASDescriptor<T> {}

impl<T> MCASDescriptor<T> {
    fn new(changes: Vec<(AtomicPtr<T>, *mut T, *mut T)>) -> MCASDescriptor<T> {
        MCASDescriptor {
            state: AtomicUsize::new(UNDECIDED),
            changes: changes,
        }
    }
}
