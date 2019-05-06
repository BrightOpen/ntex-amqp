use std::marker::PhantomData;

use actix_codec::{AsyncRead, AsyncWrite, Framed};
use actix_server_config::{Io, ServerConfig};
use actix_service::{NewService, Service};
use amqp_codec::protocol::{Error, Frame, ProtocolId};
use amqp_codec::{AmqpCodec, AmqpFrame, ProtocolIdCodec, ProtocolIdError, SaslFrame};
use futures::future::{err, ok, Either, FutureResult};
use futures::{Async, Future, Poll, Sink, Stream};

use crate::cell::Cell;
use crate::connection::Connection;
use crate::Configuration;

use super::dispatcher::Dispatcher;
use super::errors::HandshakeError;
use super::link::Link;
use super::sasl::{Sasl, SaslAuth};

/// Server dispatcher factory
pub struct Server<Io, F, St, S> {
    inner: Cell<Inner<Io, F, St, S>>,
}

pub(super) struct Inner<Io, F, St, S> {
    pub factory: F,
    config: Configuration,
    _t: PhantomData<(Io, St, S)>,
}

impl<Io, F, St, S> Server<Io, F, St, S>
where
    Io: AsyncRead + AsyncWrite,
    F: Service<Request = Option<SaslAuth>, Response = (St, S), Error = Error> + 'static,
    S: Service<Request = Link<St>, Response = (), Error = Error>,
{
    /// Create server dispatcher factory
    pub fn new(config: Configuration, factory: F) -> Self {
        Self {
            inner: Cell::new(Inner {
                factory,
                config,
                _t: PhantomData,
            }),
        }
    }
}

impl<Io, F, St, S> Clone for Server<Io, F, St, S> {
    fn clone(&self) -> Self {
        Server {
            inner: self.inner.clone(),
        }
    }
}

impl<T, F, St, S> NewService<ServerConfig> for Server<T, F, St, S>
where
    T: AsyncRead + AsyncWrite + 'static,
    F: Service<Request = Option<SaslAuth>, Response = (St, S), Error = Error> + 'static,
    S: Service<Request = Link<St>, Response = (), Error = Error> + 'static,
    St: 'static,
{
    type Request = Io<T>;
    type Response = ();
    type Error = ();
    type Service = ServerService<T, F, St, S>;
    type InitError = ();
    type Future = FutureResult<Self::Service, Self::InitError>;

    fn new_service(&self, _: &ServerConfig) -> Self::Future {
        ok(ServerService {
            inner: self.inner.clone(),
        })
    }
}

/// Server dispatcher
pub struct ServerService<Io, F, St, S> {
    inner: Cell<Inner<Io, F, St, S>>,
}

impl<T, F, St, S> Service for ServerService<T, F, St, S>
where
    T: AsyncRead + AsyncWrite + 'static,
    F: Service<Request = Option<SaslAuth>, Response = (St, S), Error = Error> + 'static,
    S: Service<Request = Link<St>, Response = (), Error = Error> + 'static,
    St: 'static,
{
    type Request = Io<T>;
    type Response = ();
    type Error = ();
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: Self::Request) -> Self::Future {
        let req = req.into_parts().0;

        let inner = self.inner.clone();
        Box::new(
            Framed::new(req, ProtocolIdCodec)
                .into_future()
                .map_err(|e| HandshakeError::from(e.0))
                .and_then(move |(protocol, framed)| match protocol {
                    Some(ProtocolId::Amqp) => {
                        let inner = inner;
                        Either::A(
                            framed
                                .send(ProtocolId::Amqp)
                                .map_err(|e| HandshakeError::from(e))
                                .and_then(move |framed| {
                                    let framed = framed.into_framed(AmqpCodec::new());
                                    open_connection(inner.config.clone(), framed).and_then(
                                        move |conn| {
                                            inner
                                                .get_mut()
                                                .factory
                                                .call(None)
                                                .map_err(|_| HandshakeError::Service)
                                                .map(move |(st, srv)| (st, srv, conn))
                                        },
                                    )
                                }),
                        )
                    }
                    Some(ProtocolId::AmqpSasl) => {
                        let mut inner = inner;
                        Either::B(Either::A(
                            framed
                                .send(ProtocolId::AmqpSasl)
                                .map_err(|e| HandshakeError::from(e))
                                .and_then(move |framed| {
                                    Sasl::new(
                                        &mut inner,
                                        framed.into_framed(AmqpCodec::<SaslFrame>::new()),
                                    )
                                    .and_then(
                                        move |(st, srv, framed)| {
                                            let framed = framed.into_framed(ProtocolIdCodec);
                                            handshake(inner.config.clone(), framed)
                                                .map(move |conn| (st, srv, conn))
                                        },
                                    )
                                }),
                        ))
                    }
                    Some(ProtocolId::AmqpTls) => Either::B(Either::B(err(HandshakeError::from(
                        ProtocolIdError::Unexpected {
                            exp: ProtocolId::Amqp,
                            got: ProtocolId::AmqpTls,
                        },
                    )))),
                    None => Either::B(Either::B(err(HandshakeError::Disconnected.into()))),
                })
                .map_err(|_| ())
                .and_then(|(st, srv, conn)| Dispatcher::new(conn, st, srv)),
        )
    }
}

pub fn handshake<Io>(
    cfg: Configuration,
    framed: Framed<Io, ProtocolIdCodec>,
) -> impl Future<Item = Connection<Io>, Error = HandshakeError>
where
    Io: AsyncRead + AsyncWrite + 'static,
{
    framed
        .into_future()
        .map_err(|e| HandshakeError::from(e.0))
        .and_then(move |(protocol, framed)| {
            if let Some(protocol) = protocol {
                if protocol == ProtocolId::Amqp {
                    Ok(framed)
                } else {
                    Err(ProtocolIdError::Unexpected {
                        exp: ProtocolId::Amqp,
                        got: protocol,
                    }
                    .into())
                }
            } else {
                Err(ProtocolIdError::Disconnected.into())
            }
        })
        .and_then(move |framed| {
            framed
                .send(ProtocolId::Amqp)
                .map_err(HandshakeError::from)
                .map(|framed| framed.into_framed(AmqpCodec::new()))
        })
        .and_then(move |framed| open_connection(cfg.clone(), framed))
}

pub fn open_connection<Io>(
    cfg: Configuration,
    framed: Framed<Io, AmqpCodec<AmqpFrame>>,
) -> impl Future<Item = Connection<Io>, Error = HandshakeError>
where
    Io: AsyncRead + AsyncWrite + 'static,
{
    // let time = time.clone();

    // read Open frame
    framed
        .into_future()
        .map_err(|res| HandshakeError::from(res.0))
        .and_then(|(frame, framed)| {
            if let Some(frame) = frame {
                let frame = frame.into_parts().1;
                match frame {
                    Frame::Open(open) => {
                        trace!("Got open: {:?}", open);
                        Ok((open, framed))
                    }
                    frame => Err(HandshakeError::Unexpected(frame)),
                }
            } else {
                Err(HandshakeError::Disconnected)
            }
        })
        .and_then(move |(open, framed)| {
            // confirm Open
            let local = cfg.to_open(None);
            framed
                .send(AmqpFrame::new(0, local.into()))
                .map_err(HandshakeError::from)
                .map(move |framed| Connection::new(framed, cfg.clone(), (&open).into(), None))
        })
}
