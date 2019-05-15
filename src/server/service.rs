use std::marker::PhantomData;

use actix_service::{IntoNewService, IntoService, NewService, Service, ServiceExt};
use amqp_codec::protocol::Error;
use futures::future::ok;
use futures::{Async, Future, Poll};

use super::link::Link;
use super::sasl::{no_sasl_auth, SaslAuth};
use crate::cell::Cell;

pub struct ServiceFactory;

impl ServiceFactory {
    /// Set state factory
    pub fn state<F, State, S, Param>(
        state: F,
    ) -> ServiceFactoryBuilder<
        State,
        impl Service<Request = Param, Response = State, Error = Error>,
        impl Service<Request = SaslAuth, Response = State, Error = Error>,
    >
    where
        F: IntoService<S>,
        State: 'static,
        S: Service<Request = Param, Response = State>,
        S::Error: Into<Error>,
    {
        ServiceFactoryBuilder {
            state: state.into_service().map_err(|e| e.into()),
            sasl: no_sasl_auth.into_service(),
            _t: PhantomData,
        }
    }

    /// Provide sasl auth factory
    pub fn sasl<F, S>(
        srv: F,
    ) -> ServiceFactoryBuilder<
        (),
        impl Service<Request = (), Response = (), Error = Error>,
        impl Service<Request = SaslAuth, Response = (), Error = Error>,
    >
    where
        F: IntoService<S>,
        S: Service<Request = SaslAuth, Response = ()>,
        S::Error: Into<Error>,
    {
        ServiceFactoryBuilder {
            state: (|()| ok(())).into_service(),
            sasl: srv.into_service().map_err(|e| e.into()),
            _t: PhantomData,
        }
    }

    /// Set service factory
    pub fn service<F, S>(
        st: F,
    ) -> ServiceFactoryService<
        (),
        impl NewService<
            Config = (),
            Request = Link<()>,
            Response = (),
            Error = Error,
            InitError = Error,
        >,
        impl Service<Request = (), Response = (), Error = Error>,
        impl Service<Request = SaslAuth, Response = (), Error = Error>,
    >
    where
        F: IntoNewService<S>,
        S: NewService<Config = (), Request = Link<()>, Response = ()>,
        S::Error: Into<Error>,
        S::InitError: Into<Error>,
    {
        ServiceFactoryService {
            inner: Cell::new(Inner {
                state: (|()| ok(())).into_service(),
                sasl: no_sasl_auth.into_service(),
                service: st
                    .into_new_service()
                    .map_err(|e| e.into())
                    .map_init_err(|e| e.into()),
                _t: PhantomData,
            }),
        }
    }
}

pub struct ServiceFactoryBuilder<State, StateSrv, SaslSrv> {
    state: StateSrv,
    sasl: SaslSrv,
    _t: PhantomData<(State,)>,
}

impl<State, StateSrv, SaslSrv> ServiceFactoryBuilder<State, StateSrv, SaslSrv>
where
    State: 'static,
    StateSrv: Service<Response = State, Error = Error>,
    SaslSrv: Service<Request = SaslAuth, Response = State, Error = Error>,
{
    /// Set service factory
    pub fn service<F, Srv>(
        self,
        st: F,
    ) -> ServiceFactoryService<
        State,
        impl NewService<
            Config = (),
            Request = Link<State>,
            Response = (),
            Error = Error,
            InitError = Error,
        >,
        StateSrv,
        SaslSrv,
    >
    where
        F: IntoNewService<Srv>,
        Srv: NewService<Config = (), Request = Link<State>, Response = (), InitError = Error>,
        Srv::InitError: Into<Error>,
        Srv::Error: Into<Error>,
    {
        ServiceFactoryService {
            inner: Cell::new(Inner {
                state: self.state,
                sasl: self.sasl,
                service: st
                    .into_new_service()
                    .map_err(|e| e.into())
                    .map_init_err(|e| e.into()),
                _t: PhantomData,
            }),
        }
    }

    /// Set sasl service factory
    pub fn sasl<F, SaslSrv2>(
        self,
        srv: F,
    ) -> ServiceFactoryBuilder<
        State,
        StateSrv,
        impl Service<Request = SaslAuth, Response = State, Error = Error>,
    >
    where
        F: IntoService<SaslSrv2>,
        SaslSrv2: Service<Request = SaslAuth, Response = State>,
        SaslSrv2::Error: Into<Error>,
    {
        ServiceFactoryBuilder {
            state: self.state,
            sasl: srv.into_service().map_err(|e| e.into()),
            _t: PhantomData,
        }
    }
}

pub struct ServiceFactoryService<State, Srv, StateSrv, SaslSrv> {
    inner: Cell<Inner<State, Srv, StateSrv, SaslSrv>>,
}

pub struct Inner<State, Srv, StateSrv, SaslSrv> {
    state: StateSrv,
    sasl: SaslSrv,
    service: Srv,
    _t: PhantomData<(State,)>,
}

impl<State, Srv, StateSrv, SaslSrv> Clone for ServiceFactoryService<State, Srv, StateSrv, SaslSrv> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<State, Srv, StateSrv, SaslSrv> Service for ServiceFactoryService<State, Srv, StateSrv, SaslSrv>
where
    Srv: NewService<Config = (), Request = Link<State>, Response = (), InitError = Error>,
    Srv::Future: 'static,
    StateSrv: Service<Response = State, Error = Error>,
    StateSrv::Future: 'static,
    SaslSrv: Service<Request = SaslAuth, Response = State, Error = Error>,
    SaslSrv::Future: 'static,
{
    type Request = (Option<SaslAuth>, StateSrv::Request);
    type Response = (State, Srv::Service);
    type Error = Error;
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, (req, param): (Option<SaslAuth>, StateSrv::Request)) -> Self::Future {
        let inner = self.inner.get_mut();
        if let Some(auth) = req {
            Box::new(inner.sasl.call(auth).join(inner.service.new_service(&())))
        } else {
            Box::new(inner.state.call(param).join(inner.service.new_service(&())))
        }
    }
}
