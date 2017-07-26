use std::io;

use model::*;
use error::{Error, Result};

use futures::sync::oneshot;
use futures::sync::mpsc;
use futures::{future, Future, BoxFuture, Poll, Async, StartSend, AsyncSink};
use futures::stream::{SplitStream, SplitSink, Stream, BoxStream};
use futures::sink::{Sink, BoxSink};

use tokio_core::reactor::Handle;

use websocket::{self, OwnedMessage, WebSocketError};

use serde_json;

use std::sync::{Arc, Mutex};

type WSClientBase = websocket::async::Client<Box<websocket::stream::async::Stream + Send + 'static>>;
type WSClient = WSAdapter;
type WSError = WebSocketError;

enum SendEvent {
    SendMessage(::serde_json::Value)
}

const GATEWAY_VERSION: u64 = 6;

/*
pub struct Connection {
    shutdown_req: oneshot::Sender<()>,
    thread: Option<::std::thread::JoinHandle<()>>,
    send: mpsc::Sender<SendEvent>,
    recv: mpsc::Receiver<Result<Event>>
}

struct Backend {
    gateway: String,
    token: String,
    shard_info: Option<[u8; 2]>,
    shutdown_req: BoxFuture<(), oneshot::Canceled>,
    send: mpsc::Receiver<SendEvent>,
    recv: mpsc::Sender<Result<Event>>,
    endpoint: Option<String>,
    session_id: Option<String>,
    conn: Option<(SplitStream<WSClient>, SplitSink<WSClient>)>,
    handle: Handle
}
*/

enum ConnErr {
    Recoverable(Error),
    NonRecoverable(Error)
}

struct ConnResult {
    connection: InnerConnection,
    ready_event: ReadyEvent,
    seq: u64
}

enum ConnState {
    Init(BoxFuture<ConnResult, Error>),
    Active(InnerConnection),
    Connecting(BoxFuture<ConnResult, Error>),
    Failed(Error),
    Bottom
}

impl ConnState {
    fn is_bottom(&self) -> bool {
        match *self {
            ConnState::Bottom => true,
            _ => false
        }
    }
}

enum PendingSend {
    // start_send has returned NotReady
    NotStarted(serde_json::Value),
    // start_send has acked the message but we need to poll_complete still
    Started(serde_json::Value),
    NotPending
}

#[derive(Clone)]
struct GatewayInfo {
    last_gateway_url: String,
    gateway_failures: u64
}

struct SessionInfo {
    session_id: String,
    last_seq: u64
}

struct AuthInfo {
    token: String,
    shard_info: Option<[u8; 2]>
}

pub struct Connection {
    handle: Handle,
    // Arc just so we can derive Send when boxing these up into BoxFutures
    gateway_info: Arc<Mutex<GatewayInfo>>,
    auth_info: Arc<AuthInfo>,

    // session data for session resumption
    session: Option<Arc<SessionInfo>>,

    // a single active connection
    state: ConnState,

    pending_send: PendingSend
}

impl Connection {
    pub fn new(handle: Handle, base_url: &str, token: &str, shard_info: Option<[u8; 2]>) -> Connection {
        let mut conn = Connection {
            handle: handle,
            gateway_info: Arc::new(Mutex::new(GatewayInfo {
                last_gateway_url: format!("{}?v={}", base_url, GATEWAY_VERSION),
                gateway_failures: 0
            })),
            auth_info: Arc::new(AuthInfo {
                token: String::from(token),
                shard_info: shard_info
            }),
            session: None,
            state: ConnState::Bottom,
            pending_send: PendingSend::NotPending
        };

        conn.state = conn.start_connect();

        return conn;
    }

    fn start_connect(&mut self) -> ConnState {
        use tokio_timer::Timer;
        use std::time::Duration;
        use websocket::client::builder::ClientBuilder;

        let handle = self.handle.clone();
        let is_initial = self.state.is_bottom();

        let gateway_info_ref = self.gateway_info.clone();
        let gateway_info = gateway_info_ref.lock().unwrap().clone();

        if gateway_info.gateway_failures >= 2 {
            // Too many failures with the current gateway, so we'll hit the REST API to find a new
            // one.
            panic!("TODO: Find new gateway");
        }

        let gateway_url = &gateway_info.last_gateway_url;
        let session_info = self.session.clone();
        let auth_info = self.auth_info.clone();

        let builder = ClientBuilder::new(gateway_url);
        if builder.is_err() {
            return ConnState::Connecting(future::err(Error::from(builder.err().unwrap())).boxed());
        }

        let client_future = builder.unwrap().async_connect(None, &self.handle).map_err(Error::from);

        // We're not able to Send a ClientNew, so we'll need to pass it off as its own task instead.
        let (tx, rx) = oneshot::channel();

        handle.spawn(client_future.then(|result| {
            let _ = tx.send(result);
            Ok(())
        }));

        let client_future = rx
            .map_err(|oneshot::Canceled| Error::Other("Impossible: channel canceled"))
            .and_then(|result| result) // extract inner result
            .map(|(conn, headers)| conn) // discard headers
            .map(|conn| WSAdapter::new(conn)) // set up ping/pong handling
            .and_then(|conn: WSAdapter|
                conn.into_future() // grab a single message
                    .map_err(|(e, _)| Error::from(e)) // but drop the connection on error
            )
            .and_then(|(msg, conn)| {
                let msg = match msg {
                    None => return Err(unexpected_close()),
                    Some(msg) => msg
                };

                let bytes = unpack_message(msg)?;
                decode(&GatewayEvent::decode, bytes)
                    .map(|msg| (conn, msg))
            })
            .then(move|result| {
                match result {
                    Err(e) => {
                        if is_initial {
                            // on initial connect we won't bother retrying
                            return future::err(e).boxed();
                        } else {
                            gateway_info_ref.lock().unwrap().gateway_failures += 1;
/*
                            Timer::default().sleep(Duration::from_secs(1)).then(
                                |_| { future::err(e) }
                            ).boxed()*/
                            future::err(e).boxed()
                        }
                    }
                    Ok((wsconn, GatewayEvent::Hello(interval))) => {
                        negotiate_connect(wsconn, session_info, auth_info, interval)
                    }
                    Ok((wsconn, ev)) => {
                        debug!("Unexpected event: {:?}", ev);
                        future::err(Error::Protocol("Expected Hello during handshake")).boxed()
                    }
                }
            })
            .boxed();

        ConnState::Connecting(client_future)
    }
}

impl Stream for Connection {
    type Item = Event;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let mut new_state = None;
        let mut state = ::std::mem::replace(&mut self.state, ConnState::Bottom);

        let result = match state {
            ConnState::Bottom => panic!("Attempted to read from a closed connection"),
            ConnState::Failed(ref mut err) => {
                // Steal the error for the time being, we'll replace the state later
                let err = ::std::mem::replace(err, Error::Other(""));
                new_state = Some(ConnState::Bottom);
                Err(err)
            }
            ConnState::Init(ref mut fut) => {
                match fut.poll() {
                    Err(err) => {
                        // initial connection failed, so just give up
                        new_state = Some(ConnState::Bottom);
                        Err(err)
                    }
                    Ok(Async::NotReady) => Ok(Async::NotReady),
                    Ok(Async::Ready(result)) => {
                        new_state = Some(ConnState::Active(result.connection));
                        Ok(Async::Ready(Some(Event::Ready(result.ready_event))))
                    }
                }
            }
            ConnState::Connecting(ref mut fut) => {
                match fut.poll() {
                    Err(err) => {
                        // kick another connection attempt off
                        new_state = Some(self.start_connect());
                        Ok(Async::NotReady)
                    }
                    Ok(Async::NotReady) => Ok(Async::NotReady),
                    Ok(Async::Ready(result)) => {
                        new_state = Some(ConnState::Active(result.connection));
                        Ok(Async::Ready(Some(Event::Ready(result.ready_event))))
                    }
                }
            }
            ConnState::Active(ref mut inner) => {
                //active.poll()
                panic!("active state")
            }
        };

        if new_state.is_some() {
            ::std::mem::replace(&mut self.state, new_state.unwrap());

            match result {
                // Poll again if we changed state
                Ok(Async::NotReady) => return self.poll(),
                _ => {}
            }
        } else {
            // Put our state back now that we're done borrowing it
            ::std::mem::replace(&mut self.state, state);
        }

        return result;
    }
}

fn negotiate_connect(
    wsconn: WSClient,
    session_info: Option<Arc<SessionInfo>>,
    auth_info: Arc<AuthInfo>,
    heartbeat_interval: u64
)
    -> BoxFuture<ConnResult, Error>
{
    match session_info {
        Some(_) => {
            // TODO
            negotiate_connect(wsconn, None, auth_info, heartbeat_interval)
        }
        None => {
            use futures::IntoFuture;

            // Start a connection from scratch
            let tmp: BoxFuture<WSAdapter, Error> = identify(&auth_info.token, auth_info.shard_info)
                .into_future()
                .and_then(|m| wsconn.send(OwnedMessage::Binary(m)))
                .boxed();

            tmp.and_then(move|wsconn| {
                    expect_message(wsconn, GatewayEvent::decode)
                })
                .and_then(move|(response, wsconn)| {
                    match response {
                        GatewayEvent::Dispatch(seq, Event::Ready(event)) => {
                            future::ok(ConnResult {
                                connection: InnerConnection::new(wsconn),
                                ready_event: event,
                                seq: seq
                            }).boxed()
                        }
                        GatewayEvent::InvalidateSession => {
                            let error = Error::Protocol("Invalid session during handshake. \
							Double-check your token or consider waiting 5 seconds between starting shards.");
                            future::err(error).boxed()
                        }
                        other => {
                            debug!("Unexpected event: {:?}", other);
                            return future::err(Error::Protocol("Expected Ready during handshake"))
                                .boxed()
                        }
                    }
                })
            .boxed()
        }
    }
}

fn expect_message<M, E>(conn: WSAdapter, decode: fn(serde_json::Value)->::std::result::Result<M, E>)
    -> BoxFuture<(M, WSAdapter), Error>
    where M: Send + 'static,
          E: 'static,
          Error: From<E>
{
    conn.into_future()
        .map_err(|(e, _)| e)
        .and_then(move |(m, conn)| {
            match m {
                None => future::err(unexpected_close()).boxed(),
                Some(m) => {
                    let res = unpack_message(m)
                        .and_then(|bytes|
                            serde_json::from_slice(&bytes).map_err(From::from)
                        )
                        .and_then(|m| decode(m).map_err(From::from))
                        .map(|m| (m, conn));

                    future::result(res).boxed()
                }
            }
        })
    .boxed()
}


fn unexpected_close() -> Error {
    Error::Closed(None, String::from("Unexpected close"))
}

fn unpack_message(m: OwnedMessage) -> Result<Vec<u8>> {
    match m {
        OwnedMessage::Binary(data) => Ok(data),
        OwnedMessage::Text(data) => Ok(data.into_bytes()),
        OwnedMessage::Close(None) =>
            Err(unexpected_close()),
        OwnedMessage::Close(Some(info)) =>
            Err(Error::Closed(Some(info.status_code), info.reason)),
        OwnedMessage::Ping(_) | OwnedMessage::Pong(_) =>
            Err(Error::Other("Ping/pong leaked into unpack path"))
    }
}


fn decode<T>(decoder: &Fn(serde_json::Value) -> Result<T>, bytes: Vec<u8>) -> Result<T>
{
    decoder(serde_json::from_slice(&bytes)?)
}


fn identify(token: &str, shard_info: Option<[u8; 2]>) -> Result<Vec<u8>>
{
    let mut result = json! {{
		"op": 2,
		"d": {
			"token": token,
			"properties": {
				"$os": ::std::env::consts::OS,
				"$browser": "Discord library for Rust",
				"$device": "discord-rs",
				"$referring_domain": "",
				"$referrer": "",
			},
			"large_threshold": 250,
			"compress": true,
			"v": GATEWAY_VERSION,
		}
	}};
    if let Some(info) = shard_info {
        result["shard"] = json![[info[0], info[1]]];
    }

    serde_json::to_vec(&result).map_err(Error::from)
}


// Simple adapter on top of websocket connections: Maps errors, and deals with websocket level pings
// and pongs
struct WSAdapter {
    client: WSClientBase,
    pong_state: PongState
}

impl WSAdapter {
    fn new(client: WSClientBase) -> Self {
        WSAdapter { client: client, pong_state: PongState::None }
    }

    fn advance_pong(&mut self) -> Result<()> {
        let msg = match self.pong_state {
            PongState::None => return Ok(()),
            PongState::Flushing => {
                return match self.client.poll_complete() {
                    Ok(Async::NotReady) => Ok(()),
                    Ok(Async::Ready(())) => {
                        self.pong_state = PongState::None;
                        Ok(())
                    },
                    Err(e) => Err(Error::from(e))
                }
            }
            PongState::Starting(ref msg) => msg.clone()
        };

        match self.client.start_send(msg.clone()) {
            Ok(AsyncSink::Ready) => {
                self.pong_state = PongState::Flushing;
                Ok(())
            },
            Ok(AsyncSink::NotReady(msg)) => {
                // Stay in the current state, wait until the current task is awoken again
                Ok(())
            },
            Err(e) => {
                self.pong_state = PongState::None;
                Err(Error::from(e))
            }
        }
    }
}

enum PongState {
    None,
    Starting(OwnedMessage),
    Flushing
}

impl Stream for WSAdapter {
    type Item = OwnedMessage;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.client.poll() {
            Ok(Async::NotReady) => {
                match self.advance_pong() {
                    Err(e) => Err(e),
                    Ok(_) => Ok(Async::NotReady)
                }
            },
            Ok(Async::Ready(Some(OwnedMessage::Ping(data)))) => {
                println!("DEBUG TX/PING: {:?}", data);
                let pong_msg = OwnedMessage::Pong(data);

                // If we already have a pong going, we'll (potentially) forget that and proceed with
                // this new one
                self.pong_state = PongState::Starting(pong_msg);

                // Now recurse to try to kick forward the state machine some more
                self.poll()
            },
            Ok(Async::Ready(msg)) => {
                Ok(Async::Ready(msg))
            }
            Err(e) => Err(Error::from(e))
        }
    }
}

impl Sink for WSAdapter {
    type SinkItem = OwnedMessage;
    type SinkError = Error;

    fn start_send(&mut self, item: Self::SinkItem)
        -> StartSend<Self::SinkItem, Self::SinkError>
    {
        println!("DEBUG TX: {:?}", item);
        self.client.start_send(item).map_err(Error::from)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.client.poll_complete().map_err(Error::from)
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.client.close().map_err(Error::from)
    }
}

struct InnerConnection {
    rx: BoxStream<Event, Error>,
    tx: BoxSink<OwnedMessage, Error>
}

impl InnerConnection {
    fn new(client: WSClient) -> InnerConnection {
        let (sink, source) = client.split();

        let source = Box::new(source.and_then(|result| parse_read(result)));

        InnerConnection {
            rx: source,
            tx: Box::new(sink)
        }
    }
}

fn parse_read(m: OwnedMessage) -> Result<Event> {
    panic!();
}
