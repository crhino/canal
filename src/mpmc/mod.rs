//! A multi-producer, multi-consumer channel implementation.

mod mutex_linked_list;
mod mpmc_bounded_queue;
mod channel;

pub use self::mutex_linked_list::MutexLinkedList;
pub use self::mpmc_bounded_queue::LockFreeQueue;
pub use self::channel::Failure;

use std::sync::{Arc};
use std::cell::UnsafeCell;
use self::channel::{Canal};

/// The sending-half of the mpmc channel.
pub struct Sender<T: Send> {
    inner: Arc<UnsafeCell<Canal<T>>>,
}

unsafe impl<T: Send> Send for Sender<T> {}

impl<T: Send> Sender<T> {
    /// Sends data to the channel.
    ///
    /// This method will never block, but may return an error with the value
    /// returned in the Err(..).
    pub fn send(&self, value: T) -> Result<(), T> {
        unsafe {
            (*self.inner.get()).send(value)
        }
    }
}

impl<T: Send> Clone for Sender<T> {
    fn clone(&self) -> Sender<T> {
        unsafe {
            (*self.inner.get()).clone_chan();
        }
        Sender { inner: self.inner.clone() }

    }
}

impl<T: Send> Drop for Sender<T> {
    fn drop(&mut self) {
        unsafe {
            (*self.inner.get()).drop_chan();
        }
    }
}

/// The receiving-half of the mpmc channel.
pub struct Receiver<T: Send> {
    inner: Arc<UnsafeCell<Canal<T>>>,
}

unsafe impl<T: Send> Send for Receiver<T> {}

impl<T: Send> Receiver<T> {
    /// Receive data from the channel.
    ///
    /// This method will block until either new data is sent or all senders have
    /// disconnected.
    pub fn recv(&self) -> Result<T, Failure> {
        unsafe {
            (*self.inner.get()).recv()
        }
    }
}

impl<T: Send> Clone for Receiver<T> {
    fn clone(&self) -> Receiver<T> {
        unsafe {
            (*self.inner.get()).clone_port();
        }
        Receiver { inner: self.inner.clone() }
    }
}

impl<T: Send> Drop for Receiver<T> {
    fn drop(&mut self) {
        unsafe {
            (*self.inner.get()).drop_port();
        }
    }
}

/// Create a channel pair using a lock-free queue with specified capacity.
///
/// Note: This is not ready for use in production, some bugs are still
/// being actively worked out.
pub fn mpmc_channel<T: Send>(cap: usize) -> (Sender<T>, Receiver<T>) {
    let inner = Arc::new(UnsafeCell::new(Canal::new(cap)));
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

        let mut send_vec = Vec::new();
        for i in 0..20 {
            let s = sn.clone();
            send_vec.push(thread::spawn(move || {
                assert!(s.send(i as u8).is_ok());
            }));
        }

        let mut recv_vec = Vec::new();
        for _i in 0..20 {
            let r = rc.clone();
            recv_vec.push(thread::spawn(move || {
                let popped = r.recv().unwrap();
                let mut found = false;
                for x in 0..20 {
                    if popped == x {
                        found = true
                    }
                }
                assert!(found);
            }));
        }

        for x in send_vec.into_iter().chain(recv_vec) {
            x.join().unwrap();
        }
    }
}
