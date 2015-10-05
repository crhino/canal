#![cfg(feature = "nightly")]

//! A multi-producer, multi-consumer channel implementation.

mod mutex_linked_list;
mod mpmc_bounded_queue;
mod channel;

pub use self::mutex_linked_list::MutexLinkedList;
pub use self::mpmc_bounded_queue::LockFreeQueue;

pub use self::channel::{mpmc_channel, Sender, Receiver};
