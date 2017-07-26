mod pingfilter;
#[cfg(test)]
mod mockstream;
mod serializer;
pub mod single_conn;
pub mod connection;

pub mod imports {
    pub use futures::{Stream, Sink, Future, Async, AsyncSink, BoxFuture, Poll, StartSend};
    pub use futures::sync::oneshot;
    pub use futures::future;
    pub use tokio_core::reactor::{Core, Handle};
    pub use super::internal::*;

    #[cfg(test)]
    pub use super::mockstream::*;
}

mod internal {
    use super::imports::*;
    use websocket::{OwnedMessage, WebSocketError};
    use internal::PrivateDiscord;
    use tokio_timer::Timer;

    /// The SessionInfo struct contains persistent information about a session kept across multiple
    /// connections
    pub struct SessionInfo {
        /// Discord instance that can be queried for gateway URLs.
        /// This is an Arc as we'll need to hand it off to a thread to execute calls
        pub discord: ::std::sync::Arc<::Discord>,
        /// Last known gateway URL
        pub gateway_url: Option<String>,
        /// Number of times we've failed to connect to this gateway
        pub gateway_failures: u32,
        /// Session ID, if any
        pub session_id: Option<String>,
        /// Last received event sequence number
        pub last_seq: u64,
        /// Shard info
        pub shard_info: Option<[u8; 2]>,
        /// Keepalive interval
        pub keepalive_interval: u64,
        /// Tokio timer
        pub timer: Timer
    }

    impl SessionInfo {
        pub fn token(&self) -> &str {
            self.discord.__get_token()
        }
    }

    use std::sync::Arc;
    use std::sync::Mutex;

    pub type SessionInfoRef = Arc<Mutex<SessionInfo>>;

    #[cfg(test)]
    pub fn in_task<Task>(taskfn: Task) where Task: FnOnce()->() + 'static {
        use tokio_core::reactor::Core;

        let core = Core::new().unwrap();

        core.handle().spawn_fn(move|| { taskfn(); Ok(()) });
    }

    // convenience traits for shorthand. We'll create mocks that also match this trait in tests.

    use model::GatewayEvent;
    use serde_json;
    use error::Error;
    // this trait matches websocket connections wrapped by the serializer
    pub trait Serializedish : Stream<Item=GatewayEvent, Error=Error>
                            + Sink<SinkItem=serde_json::Value, SinkError=Error>
                            + Send
    {}

    impl<T> Serializedish for T
    where T: Stream<Item=GatewayEvent, Error=Error> + Sink<SinkItem=serde_json::Value, SinkError=Error> + Send
    {}

    // this traits matches raw websockets and those wrapped by the ping filter
    pub trait WSClientish : Stream<Item=OwnedMessage, Error=WebSocketError>
    + Sink<SinkItem=OwnedMessage, SinkError=WebSocketError>
    {}

    impl<T> WSClientish for T
        where T: Stream<Item=OwnedMessage, Error=WebSocketError>
        + Sink<SinkItem=OwnedMessage, SinkError=WebSocketError>
    {}

    pub trait IntoSendable<I, E> : Future<Item=I, Error=E>
    {
        /// Take a future which may not be Send, and make it into a BoxFuture. Internally, this
        /// spawns the future as a task on the provided handle, then uses a oneshot handle to report
        /// the results.
        ///
        /// Note that this variant requires that oneshot::Canceled be convertible into the future's
        /// error type. Use box_via_err if it is not.
        fn box_via(self, handle: &Handle) -> BoxFuture<I, E>
            where E: From<oneshot::Canceled>, Self: Sized
        {
            self.box_via_err(handle, From::from(oneshot::Canceled))
        }

        fn box_via_err(self, handle: &Handle, cancel_err: E) -> BoxFuture<I, E>;

    }

    impl<I, E, F> IntoSendable<I, E> for F
        where I: 'static + Send,
              E: 'static + Send,
              F: Future<Item=I, Error=E> + 'static,
    {
        fn box_via_err(self, handle: &Handle, cancel_err: E) -> BoxFuture<I, E>
        {
            let (tx, rx) = oneshot::channel();

            handle.spawn(self.then(|result| {
                let _ = tx.send(result);
                Ok(())
            }));

            rx.map_err(|_| cancel_err).and_then(|result| result).boxed()
        }
    }
}