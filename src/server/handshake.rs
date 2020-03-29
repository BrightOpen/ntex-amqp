use ntex::service::{IntoServiceFactory, ServiceFactory};

use super::connect::ConnectAck;

pub fn handshake<Io, St, A, F>(srv: F) -> Handshake<Io, St, A>
where
    F: IntoServiceFactory<A>,
    A: ServiceFactory<Config = (), Response = ConnectAck<Io, St>>,
{
    Handshake::new(srv)
}

pub struct Handshake<Io, St, A> {
    a: A,
    _t: std::marker::PhantomData<(Io, St)>,
}

impl<Io, St, A> Handshake<Io, St, A>
where
    A: ServiceFactory<Config = ()>,
{
    pub fn new<F>(srv: F) -> Handshake<Io, St, A>
    where
        F: IntoServiceFactory<A>,
    {
        Handshake {
            a: srv.into_factory(),
            _t: std::marker::PhantomData,
        }
    }
}

impl<Io, St, A> Handshake<Io, St, A>
where
    A: ServiceFactory<Config = (), Response = ConnectAck<Io, St>>,
{
    pub fn sasl<F, B>(self, srv: F) -> ntex::util::either::Either<A, B>
    where
        F: IntoServiceFactory<B>,
        B: ServiceFactory<
            Config = (),
            Response = A::Response,
            Error = A::Error,
            InitError = A::InitError,
        >,
        B::Error: Into<ntex_amqp_codec::protocol::Error>,
    {
        ntex::util::either::Either::new(self.a, srv.into_factory())
    }
}
