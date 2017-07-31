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

    // Start the connection process, from any context (ignoring closed state etc)
    fn _reconnect(&mut self) {
        println!("!!! RECONNECT");
        self.state = ConnState::Connecting(start_connect(&self.handle, self.session_info.clone()));
    }

    fn reconnect(&mut self) {
        if let ConnState::Closed() = self.state {
            return;
        }

        self._reconnect();

        // important: We need to poll at least once to make sure we'll be notified when this finishes
        let _ = self.connection();
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
                        println!("conn ready");
                        // This overwrite would cause us problems without the little dance above
                        self.state = ConnState::Active(conn);
                        return self.connection();
                    },
                    Err(e) => {
                        // as would this reconnect
                        self.reconnect();
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
        let mut need_reconnect = false;

        if let Ok(Async::Ready(ref mut conn)) = self.connection() {
            match conn.poll() {
                Err(_) | Ok(Async::Ready(None)) => {
                    need_reconnect = true;
                    // fall through to release borrows
                },
                success => {
                    return success;
                }
            }
        }

        if need_reconnect {
            self.reconnect();
        }

        Ok(Async::NotReady)
    }
}

impl Sink for Connection {
    type SinkItem = serde_json::Value;
    type SinkError = Error;


    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        use futures::*;

        let mut need_reconnect = false;

        if let Async::Ready(ref mut conn) = self.connection()? {
            match conn.poll_complete() {
                Err(_) => {
                    need_reconnect = true;
                    // fall through to drop borrows
                },
                success => return success
            }
        }

        if need_reconnect {
            self.reconnect();
        }

        Ok(Async::NotReady)
    }

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        match self.connection() {
            Err(e) => return Err(e),
            Ok(Async::NotReady) => return Ok(AsyncSink::NotReady(item)),
            Ok(Async::Ready(ref mut conn)) => {
                match conn.start_send(item) {
                    Err(_) => {
                        // fall out of match to drop borrows so we can reconnect
                        // note that we didn't get the item back, so it'll just be lost...
                    }
                    other => { return other; }
                }
            }
        };

        self.reconnect();

        // lie and say we sent the message, because the Err didn't hand it back :(
        return Ok(AsyncSink::Ready);
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        if let ConnState::Active(mut conn) = ::std::mem::replace(&mut self.state, ConnState::Closed()) {
            conn.close();
        }

        Ok(Async::Ready(()))
    }
}