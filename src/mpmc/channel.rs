// Copyright 2014 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
//
// This file is heavily based on the rustc implementation of a multiple-producer, single-consumer
// channel. See copyright above. I have removed parts unnecessary to my uses and added those that
// I needed.

use mpmc::{LockFreeQueue};

use std::isize;
use std::thread;

use std::sync::atomic::{Ordering, AtomicIsize};

const DISCONNECTED: isize = isize::MIN;
const FUDGE: isize = 1024;

pub struct Canal<T> {
    queue: LockFreeQueue<T>,
    cnt: AtomicIsize, // How many items are on this channel

    // The number of channels which are currently using this packet.
    channels: AtomicIsize,
    // The number of ports which are currently using this packet.
    ports: AtomicIsize,

    sender_drain: AtomicIsize,
}

/// Failure modes for receiving on the port.
#[derive(Debug)]
pub enum Failure {
    /// There is nothing to recieve.
    Empty,
    /// The port is disconnected.
    Disconnected,
}

impl<T> Canal<T> {
   pub fn new(cap: usize) -> Canal<T> {
        Canal {
            queue: LockFreeQueue::with_capacity(cap),
            cnt: AtomicIsize::new(0),
            channels: AtomicIsize::new(1),
            ports: AtomicIsize::new(1),
            sender_drain: AtomicIsize::new(0),
        }
    }
}

impl<T: Send> Canal<T> {
    pub fn send(&self, t: T) -> Result<(), T> {
        // See Port::drop for what's going on
        if self.ports.load(Ordering::SeqCst) == 0 { return Err(t) }

        // Note that the multiple sender case is a little trickier
        // semantically than the single sender case. The logic for
        // incrementing is "add and if disconnected store disconnected".
        // This could end up leading some senders to believe that there
        // wasn't a disconnect if in fact there was a disconnect. This means
        // that while one thread is attempting to re-store the disconnected
        // states, other threads could walk through merrily incrementing
        // this very-negative disconnected count. To prevent senders from
        // spuriously attempting to send when the channels is actually
        // disconnected, the count has a ranged check here.
        //
        // This is also done for another reason. Remember that the return
        // value of this function is:
        //
        //  `true` == the data *may* be received, this essentially has no
        //            meaning
        //  `false` == the data will *never* be received, this has a lot of
        //             meaning
        //
        // In the SPSC case, we have a check of 'queue.is_empty()' to see
        // whether the data was actually received, but this same condition
        // means nothing in a multi-producer context. As a result, this
        // preflight check serves as the definitive "this will never be
        // received". Once we get beyond this check, we have permanently
        // entered the realm of "this may be received"
        if self.cnt.load(Ordering::SeqCst) < DISCONNECTED + FUDGE {
            return Err(t)
        }

        try!(self.queue.push(t));
        match self.cnt.fetch_add(1, Ordering::SeqCst) {
            // In this case, we have possibly failed to send our data, and
            // we need to consider re-popping the data in order to fully
            // destroy it. We must arbitrate among the multiple senders,
            // however, because the queues that we're using are
            // single-consumer queues. In order to do this, all exiting
            // pushers will use an atomic count in order to count those
            // flowing through. Pushers who see 0 are required to drain as
            // much as possible, and then can only exit when they are the
            // only pusher (otherwise they must try again).
            n if n < DISCONNECTED + FUDGE => {
                // see the comment in 'try' for a shared channel for why this
                // window of "not disconnected" is ok.
                self.cnt.store(DISCONNECTED, Ordering::SeqCst);

                if self.sender_drain.fetch_add(1, Ordering::SeqCst) == 0 {
                    loop {
                        // drain the queue, for info on the thread yield see the
                        // discussion in try_recv
                        loop {
                            match self.queue.pop() {
                                Some(..) => {}
                                None => break,
                            }
                        }
                        // maybe we're done, if we're not the last ones
                        // here, then we need to go try again.
                        if self.sender_drain.fetch_sub(1, Ordering::SeqCst) == 1 {
                            break
                        }
                    }

                    // At this point, there may still be data on the queue,
                    // but only if the count hasn't been incremented and
                    // some other sender hasn't finished pushing data just
                    // yet. That sender in question will drain its own data.
                }
            }

            n => { assert!(n >= 0) }
        }

        Ok(())
    }

    // Essentially the exact same thing as the stream decrement function.
    // Returns true if blocking should proceed.
    fn decrement(&self) {
        match self.cnt.fetch_sub(1, Ordering::SeqCst) {
            DISCONNECTED => { self.cnt.store(DISCONNECTED, Ordering::SeqCst); }
            n => {
                assert!(n >= 0);
            }
        }
    }

    pub fn recv(&mut self) -> Result<T, Failure> {
        loop {
            match self.try_recv() {
                Err(Failure::Empty) => {}
                data => { return data },
            }

            thread::yield_now();
        }
    }

    pub fn try_recv(&self) -> Result<T, Failure> {
        match self.queue.pop() {
            Some(data) => {
                self.decrement();
                Ok(data)
            }

            // See the discussion in the stream implementation for why we try
            // again.
            None => {
                match self.cnt.load(Ordering::SeqCst) {
                    n if n != DISCONNECTED => Err(Failure::Empty),
                    _ => {
                        match self.queue.pop() {
                            Some(t) => Ok(t), // Do not decrement b/c we are DISCONNECTED
                            None => Err(Failure::Disconnected),
                        }
                    }
                }
            }
        }
    }

    // Prepares this shared packet for a channel clone, essentially just bumping
    // a refcount.
    pub fn clone_chan(&self) {
        self.channels.fetch_add(1, Ordering::SeqCst);
    }

    // [@chrino]
    // Prepares this shared packet for a channel clone, essentially just bumping
    // a refcount.
    pub fn clone_port(&self) {
        self.ports.fetch_add(1, Ordering::SeqCst);
    }

    // Decrement the reference count on a channel. This is called whenever a
    // Chan is dropped and may end up waking up a receiver. It's the receiver's
    // responsibility on the other end to figure out that we've disconnected.
    pub fn drop_chan(&self) {
        match self.channels.fetch_sub(1, Ordering::SeqCst) {
            1 => {}
            n if n > 1 => return,
            n => panic!("bad number of channels left {}", n),
        }

        match self.cnt.swap(DISCONNECTED, Ordering::SeqCst) {
            DISCONNECTED => {}
            n => { assert!(n >= 0); }
        }
    }

    // See the long discussion inside of stream.rs for why the queue is drained,
    // and why it is done in this fashion.
    //
    // [@chrino]
    // Decrement ports and only drain on last port
    pub fn drop_port(&self) {
        match self.ports.fetch_sub(1, Ordering::SeqCst) {
            1 => {}
            n if n > 1 => return,
            n => panic!("bad number of channels left {}", n),
        }

        // TODO: Make sure this is ok in the multiple port case
        let mut steals = 0;
        while {
            let cnt = self.cnt.compare_and_swap(steals, DISCONNECTED, Ordering::SeqCst);
            cnt != DISCONNECTED && cnt != steals
        } {
            // See the discussion in 'try_recv' for why we yield
            // control of this thread.
            loop {
                match self.queue.pop() {
                    Some(..) => { steals += 1; }
                    None => break,
                }
            }
        }
    }
}

impl<T> Drop for Canal<T> {
    fn drop(&mut self) {
        assert_eq!(self.cnt.load(Ordering::SeqCst), DISCONNECTED);
        assert_eq!(self.channels.load(Ordering::SeqCst), 0);
        assert_eq!(self.ports.load(Ordering::SeqCst), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::Canal;
    use mpmc::mpmc_channel;
    use std::thread;
    use std::sync::{Arc, Barrier};

    #[test]
    fn test_send_recv() {
        let mut canal = Canal::new(20);

        for i in 0..20 {
            assert!(canal.send(i as u8).is_ok());
        }

        for _i in 0..20 {
            let popped = canal.recv().unwrap();
            let mut found = false;
            for x in 0..20 {
                if popped == x {
                    found = true
                }
            }
            assert!(found);
        }

        canal.drop_port();
        canal.drop_chan();
    }

    #[test]
    fn test_send_full() {
        let canal = Canal::new(2);
        assert!(canal.send(1 as u8).is_ok());
        assert!(canal.send(2 as u8).is_ok());
        assert!(canal.send(3 as u8).is_err());
        canal.drop_port();
        canal.drop_chan();
    }

    #[test]
    fn test_blocking() {
        let (sn, rc) = mpmc_channel(5);

        let barrier = Arc::new(Barrier::new(6));
        let mut recv_vec = Vec::new();
        for _i in 0..5 {
            let b = barrier.clone();
            let r = rc.clone();
            recv_vec.push(thread::spawn(move || {
                b.wait();
                let popped = r.recv().expect("Could not recv");
                let mut found = false;
                for x in 0..5 {
                    if popped == x {
                        found = true
                    }
                }
                assert!(found);
            }));
        }

        barrier.wait(); // Wait for all threads to start.
        thread::yield_now(); // yield current thread to ensure recv's block
        thread::spawn(move || {
            for i in 0..5 {
                sn.send(i as u8).unwrap();
                thread::yield_now();
            }

            for thr in recv_vec.into_iter() {
                thr.join().expect("recv thread errored");
            }
        }).join().expect("send thread errored");;
    }
}
