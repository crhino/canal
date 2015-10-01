#![cfg(feature = "nightly")]

//! A multi-producer, multi-consumer channel implementation.

mod mutex_linked_list;
mod mpmc_bounded_queue;

pub use self::mutex_linked_list::MutexLinkedList;
pub use self::mpmc_bounded_queue::LockFreeQueue;

// use std::sync::atomic::{AtomicUsize, Ordering};

struct Inner<T> {
    queue: LockFreeQueue<T>,
}

impl<T> Inner<T> {
    fn new(cap: usize) -> Inner<T> {
        Inner { queue: LockFreeQueue::with_capacity(cap) }
    }
}

impl<T> Clone for Inner<T> {
    fn clone(&self) -> Inner<T> {
        Inner { queue: self.queue.clone() }
    }
}

impl<T: Send> Inner<T> {
    fn send(&self, val: T) -> Result<(), T> {
        self.queue.push(val)
    }

    fn recv(&self) -> Option<T> {
        self.queue.pop()
    }
}

/// The sending-half of the mpmc channel.
pub struct Sender<T> {
    inner: Inner<T>,
}

impl<T: Send> Sender<T> {
    /// Sends data to the channel.
    pub fn send(&self, value: T) -> Result<(), T> {
        self.inner.send(value)
    }
}

impl<T: Send> Clone for Sender<T> {
    fn clone(&self) -> Sender<T> {
        Sender { inner: self.inner.clone() }
    }
}

/// The receiving-half of the mpmc channel.
pub struct Receiver<T> {
    inner: Inner<T>,
}

impl<T: Send> Receiver<T> {
    // TODO: Should this block?
    /// Receive data from the channel.
    ///
    /// This method does not block and will return None if no data is available.
    pub fn recv(&self) -> Option<T> {
        self.inner.recv()
    }
}

impl<T: Send> Clone for Receiver<T> {
    fn clone(&self) -> Receiver<T> {
        Receiver { inner: self.inner.clone() }
    }
}

/// Create a channel pair using a lock-free queue with specified capacity.
pub fn mpmc_channel<T: Send>(cap: usize) -> (Sender<T>, Receiver<T>) {
    let inner = Inner::new(cap);
    let sn = Sender { inner: inner.clone() };
    let rc = Receiver { inner: inner };
    (sn, rc)
}
