use std::cell::{RefCell};
use std::ptr;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

struct Node<T> {
    value: Option<T>,
    next: RefCell<*mut Node<T>>,
}

impl<T> Node<T> {
    unsafe fn new(value: T) -> *mut Node<T> {
        Box::into_raw(Box::new(Node {
            value: Some(value),
            next: RefCell::new(ptr::null_mut()),
        }))
    }
}

struct ListInner<T> {
    lock: Mutex<bool>,
    head: RefCell<*mut Node<T>>,
    tail: RefCell<*mut Node<T>>,
}

unsafe impl<T: Send> Send for ListInner<T> { }
unsafe impl<T: Send> Sync for ListInner<T> { }

impl<T> ListInner<T> {
    fn new() -> ListInner<T> {
        // Initialize a stub ptr in order to correctly set up the tail and head ptrs.
        let stub = Box::into_raw(Box::new(Node {
            value: None, next: RefCell::new(ptr::null_mut())
        }));
        ListInner {
            lock: Mutex::new(true),
            head: RefCell::new(stub),
            tail: RefCell::new(stub),
        }
    }
}

/// A mutex-locked List that is safe for push/pop on multiple threads.
pub struct MutexLinkedList<T> {
    inner: Arc<ListInner<T>>,
}

impl<T> MutexLinkedList<T> {
    /// Create a new MutexLinkedList.
    pub fn new() -> MutexLinkedList<T> {
        MutexLinkedList {
            inner: Arc::new(ListInner::new()),
        }
    }
}

impl<T: Send> MutexLinkedList<T> {
    /// Push a value onto queue.
    pub fn push(&self, value: T) {
        self.inner.push(value)
    }

    /// Pop a value off the queue.
    ///
    /// If the queue is empty, None is returned.
    pub fn pop(&self) -> Option<T> {
        self.inner.pop()
    }
}

impl<T: Send> Clone for MutexLinkedList<T> {
    fn clone(&self) -> MutexLinkedList<T> {
        MutexLinkedList { inner: self.inner.clone() }
    }
}

impl<T: Send> ListInner<T> {
    fn push(&self, value: T) {
        unsafe {
            let node = Node::new(value);

            let _lock = self.lock.lock();

            let prev = self.head.borrow().clone();
            *((*prev).next.borrow_mut().deref_mut()) = node;

            *(self.head.borrow_mut().deref_mut()) = node;
        }
    }

    fn pop(&self) -> Option<T> {
        unsafe {
            let _lock = self.lock.lock();
            let old = *(self.tail.borrow_mut().deref_mut());
            let next = *((*old).next.borrow_mut().deref_mut());

            if !next.is_null() {
                assert!((*old).value.is_none());
                assert!((*next).value.is_some());
                let ret = (*next).value.take().unwrap();
                *(self.tail.borrow_mut().deref_mut()) = next;
                let _: Box<Node<T>> = Box::from_raw(old);
                Some(ret)
            } else {
                None
            }
        }
    }
}

impl<T> Drop for ListInner<T> {
    fn drop(&mut self) {
        unsafe {
            let mut cur = *(self.head.borrow_mut().deref_mut());
            while !cur.is_null() {
                let next = *((*cur).next.borrow_mut().deref_mut());
                let _: Box<Node<T>> = Box::from_raw(cur);
                cur = next
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::MutexLinkedList;
    use std::thread::spawn;

    #[test]
    fn test_push_pop() {
        let q = MutexLinkedList::new();
        assert!(q.pop().is_none());
        q.push(1);
        q.push(2);
        assert_eq!(q.pop().unwrap(), 1);
        assert_eq!(q.pop().unwrap(), 2);
    }

    #[test]
    fn test_concurrent() {
        let q = MutexLinkedList::new();
        let mut guard_vec = Vec::new();
        for i in 0..10 {
            let qu = q.clone();
            guard_vec.push(spawn(move || {
                qu.push(i as u8);
            }));
        }

        for x in guard_vec.into_iter() {
            x.join().unwrap();
        }

        guard_vec = Vec::new();
        for _i in 0..10 {
            let qu = q.clone();
            guard_vec.push(spawn(move || {
                let popped = qu.pop().unwrap();
                let mut found = false;
                for x in 0..10 {
                    if popped == x {
                        found = true
                    }
                }
                assert!(found);
            }));
        }

        for thr in guard_vec.into_iter() {
            thr.join().unwrap();
        }
    }
}
