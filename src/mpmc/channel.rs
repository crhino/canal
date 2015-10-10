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

use std::cmp;
use std::isize;

use std::sync::atomic::{Ordering, AtomicIsize};
use std::sync::{Condvar, Mutex, MutexGuard};

const DISCONNECTED: isize = isize::MIN;
const FUDGE: isize = 1024;
#[cfg(test)]
const MAX_STEALS: isize = 5;
#[cfg(not(test))]
const MAX_STEALS: isize = 1 << 20;

pub struct Canal<T> {
    queue: LockFreeQueue<T>,
    cnt: AtomicIsize, // How many items are on this channel
    // TODO: Make this atomic since we now have many receivers
    steals: isize, // How many times has a port received without blocking?

    should_block: Mutex<bool>, // tells ports that they should block
    block_condvar: Condvar, // CondVar associated with should_block, how send() wakes up blocked ports

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
            steals: 0,
            should_block: Mutex::new(false),
            block_condvar: Condvar::new(),
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
            // Because multiple consumers and be decrementing cnt at once,
            // we do a ranged check here as well. We use the same fudge as
            // the DISCONNECT case in order to make sure the two cases do
            // not end up colliding with each other.
            //
            // The two cases are inherently racy, so we cannot ensure exact
            // numbers for the cnt in either case, thus we keep them far apart.
            //
            // TODO: This in essence limits the number of ports we can have. We would
            // like to have an arbitrary amount.
            n if n <= -1 && n > -1 - FUDGE  => {
                self.wake_port();
            }

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

            // Can't make any assumptions about this case like in the SPSC case.
            _ => {}
        }

        Ok(())
    }

    pub fn recv(&mut self) -> Result<T, Failure> {
        // This code is essentially the exact same as that found in the stream
        // case (see stream.rs)
        match self.try_recv() {
            Err(Failure::Empty) => {}
            data => return data,
        }

        if let Some((mut should_block, condvar)) = self.decrement() {
            while *should_block {
                should_block = condvar.wait(should_block).unwrap();
            }
        }

        match self.try_recv() {
            data @ Ok(..) => { self.steals -= 1; data }
            data => data,
        }
    }

    // Essentially the exact same thing as the stream decrement function.
    // Returns Some((guard, condvar)) if blocking should proceed.
    fn decrement<'a>(&'a mut self) -> Option<(MutexGuard<'a, bool>, &'a Condvar)> {
        let mut block = self.should_block.lock().expect("Lock was posioned");

        let steals = self.steals;
        self.steals = 0;

        // A -1 cnt value means that we need to wake someone up/block
        match self.cnt.fetch_sub(1 + steals, Ordering::SeqCst) {
            DISCONNECTED => { self.cnt.store(DISCONNECTED, Ordering::SeqCst); }
            // If we factor in our steals and notice that the channel has no
            // data, we successfully sleep
            n => {
                assert!(n > -1 - FUDGE); // See send() for more details.
                if n - steals <= 0 {
                    *block = true;
                    return Some((block, &self.block_condvar))
                }
            }
        }

        None
    }

    pub fn try_recv(&mut self) -> Result<T, Failure> {
        match self.queue.pop() {
            // See the discussion in the stream implementation for why we
            // might decrement steals.
            Some(data) => {
                if self.steals > MAX_STEALS {
                    match self.cnt.swap(0, Ordering::SeqCst) {
                        DISCONNECTED => {
                            self.cnt.store(DISCONNECTED, Ordering::SeqCst);
                        }
                        n => {
                            let m = cmp::min(n, self.steals);
                            self.steals -= m;
                            self.bump(n - m);
                        }
                    }
                    assert!(self.steals >= 0);
                }
                self.steals += 1;
                Ok(data)
            }

            // See the discussion in the stream implementation for why we try
            // again.
            None => {
                match self.cnt.load(Ordering::SeqCst) {
                    n if n != DISCONNECTED => Err(Failure::Empty),
                    _ => {
                        match self.queue.pop() {
                            Some(t) => Ok(t),
                            None => Err(Failure::Disconnected),
                        }
                    }
                }
            }
        }
    }

    fn bump(&self, amt: isize) -> isize {
        match self.cnt.fetch_add(amt, Ordering::SeqCst) {
            DISCONNECTED => {
                self.cnt.store(DISCONNECTED, Ordering::SeqCst);
                DISCONNECTED
            }
            n => n
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
            -1 => { self.wake_port(); }
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

        let mut steals = self.steals;
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

    // Set the should_block mutex to false and notify a single port. The corresponding port
    // being woken should ensure that should_block is set correctly after being woken up and
    // responding to changes.
    fn wake_port(&self) {
        let mut block = self.should_block.lock().unwrap();
        *block = false;
        self.block_condvar.notify_one();
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
                let popped = r.recv().unwrap();
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
        thread::yield_now(); // yeild current thread to ensure recv's block
        for i in 0..5 {
            sn.send(i as u8).unwrap();
        }

        for thr in recv_vec.into_iter() {
            thr.join().unwrap();
        }
    }
}
