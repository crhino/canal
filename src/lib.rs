/// A Single-Producer, Multiple-Consumer queue.

use std::sync::mpsc::{channel, Receiver, RecvError, Sender, SendError};
use std::sync::{Arc, RwLock, RwLockReadGuard};
use std::fmt;
use std::any::Any;
use std::error::Error;

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum BroadcastError<T> {
    SendError(T),
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

pub struct Broadcast<T> {
    inner: Arc<Inner<T>>,
}

impl<T: Clone> Broadcast<T> {
    pub fn send(&self, data: T) -> Result<(), BroadcastError<T>> {
        let guard = self.inner.read_senders();
        for s in guard.iter() {
            try!(s.send(data.clone()));
        }

        Ok(())
    }
}

struct Inner<T> {
    senders: RwLock<Vec<Sender<T>>>,
}

impl<T> Inner<T> {
    fn read_senders<'a>(&'a self) -> RwLockReadGuard<'a, Vec<Sender<T>>> {
        self.senders.read().unwrap()
    }

    fn add_sender(&self, sender: Sender<T>) {
        let mut vec = self.senders.write().unwrap();
        vec.push(sender);
    }
}

pub struct Consumer<T> {
    inner: Arc<Inner<T>>,
    receiver: Receiver<T>,
}

impl<T> Consumer<T> {
    fn recv(&self) -> Result<T, BroadcastError<T>> {
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

pub fn broadcast_channel<T: Clone>() -> (Broadcast<T>, Consumer<T>) {
    let (b, c) = channel();
    let inner = Arc::new(Inner { senders: RwLock::new(vec!(b)) });
    let broadcast = Broadcast { inner: inner.clone() };
    let consumer = Consumer { inner: inner, receiver: c };
    (broadcast, consumer)
}

#[cfg(test)]
mod test {
    use broadcast_channel;
    use super::Inner;

    use std::sync::{Arc, RwLock};
    use std::sync::mpsc::{channel};

    #[test]
    fn inner_iterator() {
        let (s1, r1) = channel();
        let (s2, r2) = channel();
        let inner = Arc::new(Inner { senders: RwLock::new(vec!(s1, s2)) });
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
}
