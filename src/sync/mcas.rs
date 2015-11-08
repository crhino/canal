//! A multi-word compare-and-swap implementation
//!
//! Based on paper by Keir Fraser et al.

use sync::ccas::{CondAtomicPtr};
use sync::utils::{Descriptor};
use std::sync::atomic::{AtomicPtr, Ordering, AtomicUsize};
use std::fmt;
use std::error::Error;

const UNDECIDED: usize = 0;
const FAILED: usize = 1;
const SUCCEEDED: usize = 2;

struct MCASDescriptor<'a, T: 'a> {
    state: AtomicUsize,
    changes: Vec<(&'a CondAtomicPtr<'a, T>, *mut T, *mut T)>, // (atomic, current, new) tuples
}

impl<'a, T> Descriptor<T> for MCASDescriptor<'a, T> {}

impl<'a, T> MCASDescriptor<'a, T> {
    fn new(changes: Vec<(&'a CondAtomicPtr<'a, T>, *mut T, *mut T)>) -> MCASDescriptor<T> {
        MCASDescriptor {
            state: AtomicUsize::new(UNDECIDED),
            changes: changes,
        }
    }
}

pub struct McasPtrs<T> {
    ptrs: Vec<AtomicPtr<T>>,
}

impl<T> McasPtrs<T> {
    pub fn new(words: Vec<AtomicPtr<T>>) -> McasPtrs<T> {
        McasPtrs{
            ptrs: words,
        }
    }

    pub fn compare_and_swap<T>(&self,
                               expected: Vec<*mut T>,
                               update: Vec<*mut T>) -> Result<(), MCASError> {
        match (words.len(), expected.len(), update.len()) {
            (x, y, z) if x == y && x == z => {},
            (_, _, _) => return Err(MCASError::UnequalLengths),
        }

        let cond_words = words.into_iter().map(|w| CondAtomicPtr::new(w));
        let changes: Vec<(_, _, _)> = cond_words.into_iter()
            .zip(expected.into_iter())
            .zip(update.into_iter())
            .map(|((c, e), n)| (c, e, n))
            .collect();
        Ok(())
    }
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MCASError {
    UnequalLengths,
    Other,
}

impl fmt::Display for MCASError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            MCASError::UnequalLengths =>
                write!(fmt, "received vectors of unequal lengths"),
            MCASError::Other =>
                write!(fmt, "other"),
        }
    }
}


impl Error for MCASError {
    fn description(&self) -> &str {
        match *self {
            MCASError::UnequalLengths => "received vectors of unequal lengths",
            MCASError::Other => "other",
        }
    }

    fn cause(&self) -> Option<&Error> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicPtr, Ordering};
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    struct NoCopy(usize);

    #[test]
    fn test_mcas_success() {
        let mut val1 = NoCopy(1);
        let mut val2 = NoCopy(2);
        let atm1 = AtomicPtr::new(&mut val1);
        let atm2 = AtomicPtr::new(&mut val2);

        let vec = vec!(&atm1, &atm2);
        let expected = vec!(atm1.load(Ordering::SeqCst), atm2.load(Ordering::SeqCst));

        let mut val3 = NoCopy(3);
        let mut val4 = NoCopy(4);
        let new: Vec<*mut NoCopy> = vec!(&mut val3, &mut val4);

        assert!(multi_word_compare_and_swap(vec, expected, new).is_ok());

        unsafe {
            let ptr1 = atm1.load(Ordering::SeqCst);
            let ptr2 = atm2.load(Ordering::SeqCst);

            assert!(!ptr1.is_null());
            assert!(!ptr2.is_null());

            assert_eq!(*ptr1, NoCopy(3));
            assert_eq!(*ptr2, NoCopy(4));
        }
    }

    #[test]
    fn test_mcas_unequal_lengths() {
        let mut val1 = NoCopy(1);
        let mut val2 = NoCopy(2);
        let atm1 = AtomicPtr::new(&mut val1);
        let atm2 = AtomicPtr::new(&mut val2);

        let vec = vec!(&atm1, &atm2);
        let expected = vec!(atm1.load(Ordering::SeqCst), atm2.load(Ordering::SeqCst));

        let mut val3 = NoCopy(3);
        let val4 = NoCopy(4);
        let new: Vec<*mut NoCopy> = vec!(&mut val3);

        let res = multi_word_compare_and_swap(vec, expected, new);
        match res {
            Ok(_) => assert!(false, "received Ok value, expected error"),
            Err(MCASError::UnequalLengths) => {},
            Err(e) => assert!(false, format!("received wrong error type {:?}", e)),
        }
    }
}
