use std::mem;

/// Used in the transmute functions below
pub trait Descriptor<T> {}

/// Here we need to decide whether the pointer specified is a ptr to a real T object
/// or to a descriptor. This is done by using the one low-order bit of the ptr.
///
/// If that bit is set to 1, than the ptr is a descriptor, if it is 0, then it is not.
#[inline]
pub fn is_descriptor<T>(ptr: *mut T) -> bool {
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

/*
 * This next two functions should only be used on pointers **known** to be Descriptor<T>
 * values that have been or are about to be transumted to T pointers in order to hide inside
 * of their pointer type.
 *
 * TODO: Is there a way to not have to do this crazy pointer shenanigans?
 */
#[inline]
pub unsafe fn transmute_t_to_desc<T, D>(desc: *mut T) -> *mut D where D: Descriptor<T> {
    mem::transmute(unset_descriptor(desc))
}

#[inline]
pub unsafe fn transmute_desc_to_t<T, D>(desc: *mut D) -> *mut T where D: Descriptor<T> {
    mem::transmute(set_descriptor(desc))
}
