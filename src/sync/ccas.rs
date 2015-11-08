//! A Conditional Compare-and-Swap implementation
//!
//! A more specialized version of a LL/SC or DCAS, a CCAS will do a CAS only if
//! the specified atomic evaluates to 0.

use std::sync::atomic::{Ordering, AtomicUsize, AtomicPtr};
use std::sync::{Arc};
use std::ptr;

use sync::utils::*;

struct CCASDescriptor<T> {
    current: *mut T,
    new: *mut T,
    cond: Arc<AtomicUsize>,
}

impl<T> Descriptor<T> for CCASDescriptor<T> {}

impl<T> CCASDescriptor<T> {
    fn new(current: *mut T, new: *mut T, cond: Arc<AtomicUsize>) -> CCASDescriptor<T> {
        CCASDescriptor {
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

pub struct CondAtomicPtr<'a, T: 'a> {
    inner: &'a AtomicPtr<T>,
}

impl<'a, T> CondAtomicPtr<'a, T> {
    pub fn new(val: &'a AtomicPtr<T>) -> CondAtomicPtr<T> {
        CondAtomicPtr {
            inner: val,
        }
    }

    pub fn conditional_compare_and_swap(&self,
                                        current: *mut T,
                                        new: *mut T,
                                        cond: Arc<AtomicUsize>,
                                        ordering: Ordering) -> *mut T {
        let desc = &mut CCASDescriptor::new(current, new, cond);
        let curr = desc.current_value();
        let desc = unsafe {
            transmute_desc_to_t(desc)
        };

        let mut prev = ptr::null_mut();
        while {
            prev = self.inner.compare_and_swap(curr, desc, ordering);
            prev != curr
        } {
            if !is_descriptor(prev) { return prev };
            self.ccas_help(prev, ordering);
        }

        self.ccas_help(desc, ordering);
        prev
    }

    pub fn load(&self, ordering: Ordering) -> *mut T {
        let mut desc = self.inner.load(ordering);
        while is_descriptor(desc) {
            self.ccas_help(desc, Ordering::SeqCst);
            desc = self.inner.load(ordering);
        }

        desc
    }

    fn ccas_help(&self, desc: *mut T, ordering: Ordering) {
        unsafe {
            let desc: *mut CCASDescriptor<T> = transmute_t_to_desc(desc);
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

#[cfg(test)]
mod tests {
    use crossbeam;
    use std::sync::{Arc};
    use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
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
        let atm = AtomicPtr::new(&mut val);
        let ptr = CondAtomicPtr::new(&atm);

        let mut new_val = Test::new(1, 2);
        let cond = Arc::new(AtomicUsize::new(0));

        let old = ptr.conditional_compare_and_swap(&mut val,
                                                   &mut new_val,
                                                   cond, Ordering::SeqCst);
        let data = unsafe { (*old).data() };
        assert_eq!(data, (27, 36));

        let new_ptr = ptr.load(Ordering::SeqCst);
        let data = unsafe { (*new_ptr).data() };
        assert_eq!(data, (1, 2));
    }

    #[test]
    fn test_ccas_cond_false() {
        let mut val = Test::new(27, 36);
        let atm = AtomicPtr::new(&mut val);
        let ptr = CondAtomicPtr::new(&atm);

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
        let atm = AtomicPtr::new(&mut val);
        let ptr = CondAtomicPtr::new(&atm);
        let cond = Arc::new(AtomicUsize::new(0));
        let val_ptr = (&val as *const Test) as usize;

        crossbeam::scope(|scope| {
            for _i in 0..20 {
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
