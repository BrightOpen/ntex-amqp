use futures::future::{err, ok};
use ntex::codec::{AsyncRead, AsyncWrite};
use ntex::framed::{Dispatcher as IoDispatcher, State as IoState, Timer};
use ntex::service::{fn_service, Service};

use crate::codec::{AmqpCodec, AmqpFrame};
use crate::error::{DispatcherError, LinkError};
use crate::{dispatcher::Dispatcher, Configuration, Connection, State};

/// Mqtt client
pub struct Client<Io, St = ()> {
    io: Io,
    state: IoState,
    codec: AmqpCodec<AmqpFrame>,
    connection: Connection,
    keepalive: u16,
    remote_config: Configuration,
    timer: Timer,
    st: State<St>,
}

impl<T> Client<T, ()>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    /// Construct new `Dispatcher` instance with outgoing messages stream.
    pub(super) fn new(
        io: T,
        state: IoState,
        codec: AmqpCodec<AmqpFrame>,
        connection: Connection,
        keepalive: u16,
        remote_config: Configuration,
        timer: Timer,
    ) -> Self {
        Client {
            io,
            state,
            codec,
            connection,
            keepalive,
            remote_config,
            timer,
            st: State::new(()),
        }
    }
}

impl<Io, St> Client<Io, St>
where
    St: 'static,
    Io: AsyncRead + AsyncWrite + Unpin + 'static,
{
    #[inline]
    /// Get client sink
    pub fn sink(&self) -> Connection {
        self.connection.clone()
    }

    #[inline]
    /// Set connection state
    pub fn state<T: 'static>(self, st: T) -> Client<Io, T> {
        Client {
            io: self.io,
            state: self.state,
            codec: self.codec,
            connection: self.connection,
            keepalive: self.keepalive,
            remote_config: self.remote_config,
            timer: self.timer,
            st: State::new(st),
        }
    }

    /// Run client with default control messages handler.
    ///
    /// Default handler closes connection on any control message.
    pub async fn start_default(self) -> Result<(), DispatcherError> {
        let dispatcher = Dispatcher::new(
            self.st,
            self.connection,
            fn_service(|_| err::<_, LinkError>(LinkError::force_detach())),
            fn_service(|_| ok::<_, LinkError>(())),
            self.remote_config.timeout_remote_secs(),
        )
        .map(|_| Option::<AmqpFrame>::None);

        IoDispatcher::new(self.io, self.codec, self.state, dispatcher, self.timer)
            .keepalive_timeout(if self.keepalive != 0 {
                self.keepalive + 5
            } else {
                0
            })
            .await
    }
}
