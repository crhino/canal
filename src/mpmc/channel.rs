//! A module containing the "port" and "channel" implementation of a multi-thread
//! message queue.

use mpmc::{LockFreeQueue};
use std::sync::{Arc};
// use std::sync::atomic::{AtomicUsize, Ordering};

struct Inner<T> {
    queue: LockFreeQueue<T>,
}

impl<T> Inner<T> {
    fn new(cap: usize) -> Inner<T> {
        Inner { queue: LockFreeQueue::with_capacity(cap) }
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
    inner: Arc<Inner<T>>,
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
    inner: Arc<Inner<T>>,
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
    let inner = Arc::new(Inner::new(cap));
    let sn = Sender { inner: inner.clone() };
    let rc = Receiver { inner: inner };
    (sn, rc)
}

#[cfg(test)]
mod tests {
    use std::thread;
    use mpmc::{mpmc_channel};

    #[test]
    fn test_producer_consumer() {
        let (sn, rc) = mpmc_channel(25);

        let mut guard_vec = Vec::new();
        for i in 0..10 {
            let sn = sn.clone();
            guard_vec.push(thread::spawn(move || {
                assert!(sn.send(i as u8).is_ok());
            }));
        }

        for x in guard_vec.into_iter() {
            x.join().unwrap();
        }

        guard_vec = Vec::new();
        for _i in 0..10 {
            let rc = rc.clone();
            guard_vec.push(thread::spawn(move || {
                let popped = rc.recv().unwrap();
                let mut found = false;
                for x in 0..10 {
                    if popped == x {
                        found = true
                    }
                }
                assert!(found);
            }));
        }

        for x in guard_vec.into_iter() {
            x.join().unwrap();
        }
    }
}
