use std::time::{Duration, Instant};

use actix_utils::time::LowResTimeService;
use futures::{Async, Future};
use tokio_timer::Delay;

use crate::errors::AmqpTransportError;

pub(crate) enum HeartbeatAction {
    None,
    Heartbeat,
    Close,
}

pub(crate) struct Heartbeat {
    expire_local: Instant,
    expire_remote: Instant,
    local: Duration,
    remote: Option<Duration>,
    time: LowResTimeService,
    delay: Delay,
}

impl Heartbeat {
    pub(crate) fn new(local: Duration, remote: Option<Duration>, time: LowResTimeService) -> Self {
        let now = time.now();
        let delay = if let Some(remote) = remote {
            Delay::new(now + std::cmp::min(local, remote))
        } else {
            Delay::new(now + local)
        };

        Heartbeat {
            expire_local: now,
            expire_remote: now,
            local,
            remote,
            time,
            delay,
        }
    }

    pub(crate) fn update_local(&mut self, update: bool) {
        if update {
            self.expire_local = self.time.now();
        }
    }

    pub(crate) fn update_remote(&mut self, update: bool) {
        if update && self.remote.is_some() {
            self.expire_remote = self.time.now();
        }
    }

    fn next_expire(&self) -> Instant {
        if let Some(remote) = self.remote {
            std::cmp::min(self.expire_local + self.local, self.expire_remote + remote)
        } else {
            self.expire_local + self.local
        }
    }

    pub(crate) fn poll(&mut self) -> Result<HeartbeatAction, AmqpTransportError> {
        match self.delay.poll() {
            Ok(Async::Ready(_)) => {
                let mut act = HeartbeatAction::None;
                let dl = self.delay.deadline();
                if dl >= self.expire_local + self.local {
                    // close connection
                    return Ok(HeartbeatAction::Close);
                }
                if let Some(remote) = self.remote {
                    if dl >= self.expire_remote + remote {
                        // send heartbeat
                        act = HeartbeatAction::Heartbeat;
                    }
                }
                self.delay.reset(self.next_expire());
                let _ = self.delay.poll();
                Ok(act)
            }
            Ok(Async::NotReady) => Ok(HeartbeatAction::None),
            Err(e) => {
                error!("Tokio timer error: {}", e);
                Err(AmqpTransportError::Timeout)
            }
        }
    }
}
