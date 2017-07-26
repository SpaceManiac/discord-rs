use super::imports::*;
use super::single_conn::{SingleConnection, start_connect};
use error::Error;
use std::sync::{Arc,Mutex};
use Discord;
use model::Event;
use serde_json;

pub struct Connection {
    handle: Handle,
    session_info: SessionInfoRef,
    state: ConnState
}

enum ConnState {
    Connecting(BoxFuture<SingleConnection, Error>),
    Active(SingleConnection),
    Closed()
}

impl Connection {
    pub fn new(discord: Discord, handle: &Handle) -> Self {
        let mut conn = Connection {
            handle: handle.clone(),
            session_info: Arc::new(Mutex::new(
                SessionInfo {
                    discord: Arc::new(discord),
                    gateway_url: None,
                    gateway_failures: 0,
                    session_id: None,
                    last_seq: 0,
                    shard_info: None,
                    keepalive_interval: 0,
                    timer: ::tokio_timer::Timer::default()
                }
            )),
            state: ConnState::Closed()
        };

        conn._reconnect();

        return conn;
    }

    fn _reconnect(&mut self) {
        println!("!!! RECONNECT");
        self.state = ConnState::Connecting(start_connect(&self.handle, self.session_info.clone()));
    }

    fn reconnect(&mut self) {
        self._reconnect();
    }

    fn connection(&mut self) -> Poll<&mut SingleConnection, Error> {
        match self.state {
            ConnState::Active(ref mut conn) => {
                return Ok(Async::Ready(conn));
            },
            ConnState::Connecting(_) => {
                return self.finish_connect();
            },
            ConnState::Closed() => {
                return Err(Error::Other("Attempted to use a closed connection"))
            }
        }
    }

    fn finish_connect(&mut self) -> Poll<&mut SingleConnection, Error> {
        // Move the future out of our state field so we can (potentially) replace it
        let mut state = ::std::mem::replace(&mut self.state, ConnState::Closed());
        match state {
            ConnState::Connecting(ref mut f) => {
                match f.poll() {
                    Ok(Async::Ready(conn)) => {
                        // This overwrite would cause us problems without the little dance above
                        self.state = ConnState::Active(conn);
                        return self.connection();
                    },
                    Err(e) => {
                        // as would this reconnect
                        self._reconnect();
                        return Ok(Async::NotReady);
                    },
                    Ok(Async::NotReady) => {
                        /* fall through to release borrows on state */
                    },
                }
            }
            _ => unreachable!()
        }

        // If we fell through to here, it means the future wasn't ready, so we need to put it back
        // into the state field.
        self.state = state;

        Ok(Async::NotReady)
    }
}

impl Stream for Connection {
    type Item = Event;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if let Ok(Async::Ready(ref mut conn)) = self.connection() {
            return conn.poll();
        }

        Ok(Async::NotReady)
    }
}

impl Sink for Connection {
    type SinkItem = serde_json::Value;
    type SinkError = Error;


    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        use futures::*;

        if let Async::Ready(ref mut conn) = self.connection()? {
            return conn.poll_complete()
        }

        Ok(Async::NotReady)
    }

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        match self.connection() {
            Err(e) => Err(e),
            Ok(Async::NotReady) => Ok(AsyncSink::NotReady(item)),
            Ok(Async::Ready(ref mut conn)) => conn.start_send(item)
        }
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        if let ConnState::Active(mut conn) = ::std::mem::replace(&mut self.state, ConnState::Closed()) {
            conn.close();
        }

        Ok(Async::Ready(()))
    }
}