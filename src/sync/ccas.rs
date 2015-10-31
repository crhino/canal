//! A Conditional Compare-and-Swap implementation
//!
//! A more specialized version of a LL/SC or DCAS, a CCAS will do a CAS only if
//! the specified atomic evaluates to 0.

use std::sync::atomic::{Ordering, AtomicUsize, AtomicPtr};
use std::sync::{Arc};
use std::mem;

struct Descriptor<T> {
    current: *mut T,
    new: *mut T,
    cond: Arc<AtomicUsize>,
}

impl<T> Descriptor<T> {
    fn new(current: *mut T, new: *mut T, cond: Arc<AtomicUsize>) -> Descriptor<T> {
        Descriptor {
            current: current,
            new: new,
            cond: cond,
        }
    }

    fn current_value(&self) -> *mut T {
        self.current
    }

    fn new_value(&self) -> *mut T {
        self.new
    }

    fn load_condition(&self, ordering: Ordering) -> usize {
        self.cond.load(ordering)
    }
}

pub struct CondAtomicPtr<T> {
    inner: AtomicPtr<T>,
}

impl<T> CondAtomicPtr<T> {
    pub fn new(val: *mut T) -> CondAtomicPtr<T> {
        CondAtomicPtr {
            inner: AtomicPtr::new(val),
        }
    }

    /// No return value, read Section 5.4 of paper for more background
    pub fn conditional_compare_and_swap(&self,
                                        current: *mut T,
                                        new: *mut T,
                                        cond: Arc<AtomicUsize>,
                                        ordering: Ordering) {
        let desc = &mut Descriptor::new(current, new, cond);
        let curr = desc.current_value();
        let desc = unsafe {
            transmute_desc_to_t(desc)
        };

        loop {
            let prev = self.inner.compare_and_swap(curr, desc, ordering);
            if prev == curr { break };
            if !is_descriptor(prev) { return };
        }

        self.ccas_help(desc, ordering);
    }

    pub fn load(&self, ordering: Ordering) -> *mut T {
        loop {
            let desc = self.inner.load(ordering);
            if !is_descriptor(desc) { return desc };
            self.ccas_help(desc, Ordering::SeqCst);
        }

        unreachable!();
    }

    fn ccas_help(&self, desc: *mut T, ordering: Ordering) {
        unsafe {
            let desc = transmute_t_to_desc(desc);
            let cas_val = if (*desc).load_condition(Ordering::SeqCst) == 0 {
                (*desc).new_value()
            } else {
                (*desc).current_value()
            };

            let desc = transmute_desc_to_t(desc);
            self.inner.compare_and_swap(desc, cas_val, ordering);
        }
    }
}

/// Here we need to decide whether the pointer specified is a ptr to a real T object
/// or to a descriptor. This is done by using the one low-order bit of the ptr.
///
/// If that bit is set to 1, than the ptr is a descriptor, if it is 0, then it is not.
#[inline]
fn is_descriptor<T>(ptr: *mut T) -> bool {
    ((ptr as usize) & 1) == 1
}

/// Set the 0th bit of the pointer to 1 in order to represent it as a descriptor value
#[inline]
unsafe fn set_descriptor<T>(ptr: *mut T) -> *mut T {
    assert!(!is_descriptor(ptr));
    ((ptr as usize) | 1) as *mut T
}

/// Set the 0th bit of the pointer to 0 in order to represent it as a ptr to T value
#[inline]
unsafe fn unset_descriptor<T>(ptr: *mut T) -> *mut T {
    assert!(is_descriptor(ptr));
    (((ptr as usize) >> 1) << 1) as *mut T
}

#[inline]
unsafe fn transmute_t_to_desc<T>(desc: *mut T) -> *mut Descriptor<T> {
    mem::transmute(unset_descriptor(desc))
}

#[inline]
unsafe fn transmute_desc_to_t<T>(desc: *mut Descriptor<T>) -> *mut T {
    mem::transmute(set_descriptor(desc))
}

#[cfg(test)]
mod tests {
    use crossbeam;
    use std::sync::{Arc};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use super::CondAtomicPtr;
    struct Test {
        data: usize,
        other_data: usize,
    }

    impl Test {
        fn new(data: usize, other_data: usize) -> Test {
            Test {
                data: data,
                other_data: other_data,
            }
        }

        fn data(&self) -> (usize, usize) {
            (self.data, self.other_data)
        }
    }

    #[test]
    fn test_ccas() {
        let mut val = Test::new(27, 36);
        let ptr = CondAtomicPtr::new(&mut val);

        let mut new_val = Test::new(1, 2);
        let cond = Arc::new(AtomicUsize::new(0));

        ptr.conditional_compare_and_swap(&mut val, &mut new_val, cond, Ordering::SeqCst);

        let new_ptr = ptr.load(Ordering::SeqCst);
        let data = unsafe { (*new_ptr).data() };
        assert_eq!(data, (1, 2));
    }

    #[test]
    fn test_ccas_cond_false() {
        let mut val = Test::new(27, 36);
        let ptr = CondAtomicPtr::new(&mut val);

        let mut new_val = Test::new(1, 2);
        let cond = Arc::new(AtomicUsize::new(1));

        ptr.conditional_compare_and_swap(&mut val, &mut new_val, cond, Ordering::SeqCst);

        let new_ptr = ptr.load(Ordering::SeqCst);
        let data = unsafe { (*new_ptr).data() };
        assert_eq!(data, (27, 36));
    }

    #[test]
    fn test_ccas_many_threads() {
        let mut val = Test::new(27, 36);
        let ptr = CondAtomicPtr::new(&mut val);
        let cond = Arc::new(AtomicUsize::new(0));
        let val_ptr = (&val as *const Test) as usize;

        crossbeam::scope(|scope| {
            for _i in 0..50 {
                let c = cond.clone();
                scope.spawn(|| {
                    let mut new_val = Test::new(1, 1);
                    ptr.conditional_compare_and_swap(
                        val_ptr as *mut Test,
                        &mut new_val,
                        c, Ordering::SeqCst);
                    ptr.load(Ordering::SeqCst);
                });
            }
        });

        let new_ptr = ptr.load(Ordering::SeqCst);
        let data = unsafe { (*new_ptr).data() };
        assert!(data != (27, 36));
    }
}
