#![cfg(feature = "nightly")]

//! A multi-producer, multi-consumer channel implementation.

mod mutex_linked_list;
mod mpmc_bounded_queue;

use self::mutex_linked_list::MutexLinkedList;
use self::mpmc_bounded_queue::Queue;

enum QueueType<T> {
    Mutex(MutexLinkedList<T>),
    LockFree(Queue<T>),
}

/// The sending-half of the mpmc channel.
pub struct Sender<T> {
    inner: QueueType<T>,
}

impl<T: Send> Sender<T> {
    /// Sends data to the channel.
    pub fn send(&self, value: T) -> Result<(), T> {
        match self.inner {
            QueueType::Mutex(ref q) => q.push(value),
            QueueType::LockFree(ref q) => q.push(value),
        }
    }
}

impl<T: Send> Clone for Sender<T> {
    fn clone(&self) -> Sender<T> {
        let inner = match self.inner {
            QueueType::Mutex(ref q) => QueueType::Mutex(q.clone()),
            QueueType::LockFree(ref q) => QueueType::LockFree(q.clone()),
        };
        Sender { inner: inner }
    }
}

/// The receiving-half of the mpmc channel.
pub struct Receiver<T> {
    inner: QueueType<T>,
}

impl<T: Send> Receiver<T> {
    // TODO: Should this block?
    /// Receive data from the channel.
    ///
    /// This method does not block and will return None if no data is available.
    pub fn recv(&self) -> Option<T> {
        match self.inner {
            QueueType::Mutex(ref q) => q.pop(),
            QueueType::LockFree(ref q) => q.pop(),
        }
    }
}

impl<T: Send> Clone for Receiver<T> {
    fn clone(&self) -> Receiver<T> {
        let inner = match self.inner {
            QueueType::Mutex(ref q) => QueueType::Mutex(q.clone()),
            QueueType::LockFree(ref q) => QueueType::LockFree(q.clone()),
        };
        Receiver { inner: inner }
    }
}

/// Create a channel pair using a lock-free queue with specified capacity.
pub fn mpmc_channel<T: Send>(cap: usize) -> (Sender<T>, Receiver<T>) {
    let q = Queue::with_capacity(cap);
    let sn = Sender { inner: QueueType::LockFree(q.clone()) };
    let rc = Receiver { inner: QueueType::LockFree(q.clone()) };
    (sn, rc)
}

/// Create a channel pair using a mutex-locked queue.
pub fn mutex_mpmc_channel<T: Send>() -> (Sender<T>, Receiver<T>) {
    let q = MutexLinkedList::new();
    let sn = Sender { inner: QueueType::Mutex(q.clone()) };
    let rc = Receiver { inner: QueueType::Mutex(q.clone()) };
    (sn, rc)
}