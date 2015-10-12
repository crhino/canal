//! A Single-Producer, Multiple-Consumer queue.

use std::sync::mpsc::{channel, Receiver, RecvError, Sender, SendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::fmt;
use std::any::Any;
use std::error::Error;

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
/// Error from broadcast module.
pub enum BroadcastError<T> {
    /// Send error
    SendError(T),
    /// Receive error
    RecvError,
}

impl<T: fmt::Display> fmt::Display for BroadcastError<T> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            BroadcastError::SendError(ref t) =>
                write!(fmt, "could not send data on channel: {}", t),
            BroadcastError::RecvError =>
                write!(fmt, "could not receive data on channel"),
        }
    }
}


impl<T: Send + fmt::Display + fmt::Debug + Any> Error for BroadcastError<T> {
    fn description(&self) -> &str {
        match *self {
            BroadcastError::SendError(_) => "could not send data on channel",
            BroadcastError::RecvError => "could not receive data on channel",
        }
    }

    fn cause(&self) -> Option<&Error> {
        None
    }
}

impl<T> From<SendError<T>> for BroadcastError<T> {
    fn from(err: SendError<T>) -> BroadcastError<T> {
        let SendError(data) = err;
        BroadcastError::SendError(data)
    }
}

impl<T> From<RecvError> for BroadcastError<T> {
    fn from(_err: RecvError) -> BroadcastError<T> {
        BroadcastError::RecvError
    }
}

/// Struct that sends message on a broadcast pattern.
pub struct Broadcast<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Broadcast<T> {
    /// Create a new Broadcast struct.
    pub fn new() -> Broadcast<T> {
        let inner = Arc::new(Inner { senders: Mutex::new(Vec::new()) });
        Broadcast { inner: inner }
    }

    /// Create a Consumer that listens to messages from the Broadcaster.
    pub fn consume(&self) -> Consumer<T> {
        let (b, c) = channel();
        self.inner.add_sender(b);
        Consumer { inner: self.inner.clone(), receiver: c }
    }
}

impl<T: Clone> Broadcast<T> {
    /// Send a message on the broadcast.
    pub fn send(&self, data: T) -> Result<(), BroadcastError<T>> {
        let guard = self.inner.read_senders();
        for s in guard.iter() {
            try!(s.send(data.clone()));
        }

        Ok(())
    }
}

struct Inner<T> {
    senders: Mutex<Vec<Sender<T>>>,
}

impl<T> Inner<T> {
    fn read_senders<'a>(&'a self) -> MutexGuard<'a, Vec<Sender<T>>> {
        self.senders.lock().unwrap()
    }

    fn add_sender(&self, sender: Sender<T>) {
        let mut vec = self.senders.lock().unwrap();
        // Clone the sender to ensure we receive a shared::Packet that
        // doese not perform allocations on sending
        vec.push(sender.clone());
    }
}

/// Struct that receives messages from Broadcast.
pub struct Consumer<T> {
    inner: Arc<Inner<T>>,
    receiver: Receiver<T>,
}

impl<T> Consumer<T> {
    /// Receive a message from the Broadcast.
    ///
    /// This function will block.
    pub fn recv(&self) -> Result<T, BroadcastError<T>> {
        let data = try!(self.receiver.recv());
        Ok(data)
    }
}

impl<T> Clone for Consumer<T> {
    fn clone(&self) -> Self {
        let (s, r) = channel();
        self.inner.add_sender(s);
        Consumer {
            inner: self.inner.clone(),
            receiver: r,
        }
    }
}

/// Create a (Broadcast<T>, Consumer<T>) pair.
pub fn broadcast_channel<T: Clone>() -> (Broadcast<T>, Consumer<T>) {
    let broadcast = Broadcast::new();
    let consumer = broadcast.consume();
    (broadcast, consumer)
}

#[cfg(test)]
mod test {
    use broadcast::broadcast_channel;
    use super::Inner;

    use std::sync::{Arc, Mutex};
    use std::sync::mpsc::{channel};
    use std::thread::spawn;

    #[test]
    fn inner_iterator() {
        let (s1, r1) = channel();
        let (s2, r2) = channel();
        let inner = Arc::new(Inner { senders: Mutex::new(vec!(s1, s2)) });
        let guard = inner.read_senders();
        for s in guard.iter() {
            assert!(s.send(10u8).is_ok());
        }

        assert_eq!(r1.recv().unwrap(), 10u8);
        assert_eq!(r2.recv().unwrap(), 10u8);
    }

    #[test]
    fn sends_to_multiple_consumers() {
        let (p, c) = broadcast_channel();
        let c2 = c.clone();

        let res = p.send(9u8);
        assert!(res.is_ok());

        let res = c.recv();
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 9u8);

        let res = c2.recv();
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 9u8)
    }

    #[test]
    fn test_send_threads() {
        let (p, c1) = broadcast_channel();
        let c2 = c1.clone();

        let (s1, r1) = channel();
        let (s2, r2) = channel();

        let _thread = spawn(move || {
            assert_eq!(c1.recv().unwrap(), 9u8);
            s1.send(10u8).unwrap();
        });

        let _thread = spawn(move || {
            assert_eq!(c2.recv().unwrap(), 9u8);
            s2.send(10u8).unwrap();
        });

        assert!(p.send(9u8).is_ok());
        assert!(r1.recv().is_ok());
        assert!(r2.recv().is_ok());
    }
}
