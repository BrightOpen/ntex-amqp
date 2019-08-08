use std::collections::VecDeque;
use std::u32;

use amqp_codec::protocol::{Attach, Disposition, Error, LinkError, Transfer};
use amqp_codec::types::ByteStr;
use futures::task::AtomicTask;
use futures::{unsync::oneshot, Async, Future, Poll, Stream};

use crate::cell::Cell;
use crate::errors::AmqpTransportError;
use crate::session::{Session, SessionInner};
use crate::Configuration;

#[derive(Clone)]
pub struct ReceiverLink {
    inner: Cell<ReceiverLinkInner>,
}

impl ReceiverLink {
    pub(crate) fn new(inner: Cell<ReceiverLinkInner>) -> ReceiverLink {
        ReceiverLink { inner }
    }

    pub fn handle(&self) -> usize {
        self.inner.get_ref().handle
    }

    pub fn credit(&self) -> u32 {
        self.inner.get_ref().credit
    }

    pub fn session(&self) -> &Session {
        &self.inner.get_ref().session
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.inner.get_mut().session
    }

    pub fn frame(&self) -> &Attach {
        &self.inner.get_ref().attach
    }

    pub fn open(&mut self) {
        let inner = self.inner.get_mut();
        inner
            .session
            .inner
            .get_mut()
            .confirm_receiver_link(inner.handle, &inner.attach);
    }

    pub fn set_link_credit(&mut self, credit: u32) {
        self.inner.get_mut().set_link_credit(credit);
    }

    /// Send disposition frame
    pub fn send_disposition(&mut self, disp: Disposition) {
        self.inner
            .get_mut()
            .session
            .inner
            .get_mut()
            .post_frame(disp.into());
    }

    pub fn close(&mut self) -> impl Future<Item = (), Error = AmqpTransportError> {
        self.inner.get_mut().close(None)
    }

    pub fn close_with_error(
        &mut self,
        error: Error,
    ) -> impl Future<Item = (), Error = AmqpTransportError> {
        self.inner.get_mut().close(Some(error))
    }

    #[inline]
    /// Get remote connection configuration
    pub fn remote_config(&self) -> &Configuration {
        &self.inner.session.remote_config()
    }
}

impl Stream for ReceiverLink {
    type Item = Transfer;
    type Error = AmqpTransportError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let inner = self.inner.get_mut();

        if let Some(tr) = inner.queue.pop_front() {
            Ok(Async::Ready(Some(tr)))
        } else {
            if inner.closed {
                Ok(Async::Ready(None))
            } else {
                inner.reader_task.register();
                Ok(Async::NotReady)
            }
        }
    }
}

pub(crate) struct ReceiverLinkInner {
    handle: usize,
    attach: Attach,
    session: Session,
    closed: bool,
    reader_task: AtomicTask,
    queue: VecDeque<Transfer>,
    credit: u32,
    delivery_count: u32,
}

impl ReceiverLinkInner {
    pub(crate) fn new(
        session: Cell<SessionInner>,
        handle: usize,
        attach: Attach,
    ) -> ReceiverLinkInner {
        ReceiverLinkInner {
            session: Session::new(session),
            closed: false,
            reader_task: AtomicTask::new(),
            queue: VecDeque::with_capacity(4),
            credit: 0,
            delivery_count: attach.initial_delivery_count().unwrap_or(0),
            handle,
            attach,
        }
    }

    pub fn name(&self) -> &ByteStr {
        &self.attach.name
    }

    pub fn close(
        &mut self,
        error: Option<Error>,
    ) -> impl Future<Item = (), Error = AmqpTransportError> {
        let (tx, rx) = oneshot::channel();
        if self.closed {
            let _ = tx.send(Ok(()));
        } else {
            self.session
                .inner
                .get_mut()
                .detach_receiver_link(self.handle, true, error, tx);
        }
        rx.then(|res| match res {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AmqpTransportError::Disconnected),
        })
    }

    pub fn set_link_credit(&mut self, credit: u32) {
        self.credit += credit;
        self.session
            .inner
            .get_mut()
            .rcv_link_flow(self.handle as u32, self.delivery_count, credit);
    }

    pub fn handle_transfer(&mut self, transfer: Transfer) {
        if self.credit == 0 {
            // check link credit
            let err = Error {
                condition: LinkError::TransferLimitExceeded.into(),
                description: None,
                info: None,
            };
            let _ = self.close(Some(err));
        } else {
            self.credit -= 1;
            self.delivery_count += 1;
            self.queue.push_back(transfer);
            if self.queue.len() == 1 {
                self.reader_task.notify()
            }
        }
    }
}
