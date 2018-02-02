//! the core connection to discord, the heart of the bot
//! handles both api requests and gateway logic

use websocket::ClientBuilder as WSClientBuilder;
use websocket::async::Client;
use websocket::{ Message, OwnedMessage };


use reqwest::unstable::async::ClientBuilder;
use tokio_core::reactor;
use tokio_timer::{ Timer, Interval };

use websocket::async::futures::sink::With;
use websocket::async::futures::{ 
    IntoFuture, 
    Future, 
    Stream, 
    Sink, 
    Poll,
    Async,
    AsyncSink
};

use futures::sync::mpsc::{UnboundedSender, unbounded};
use std::sync::mpsc;

use flate2;
use serde_json;
use std::time::Duration;
use std::thread;

use error::Error;
use model::GatewayEvent;

const GATEWAY_VERSION: u64 = 6;

pub type GatewayNew = Box<Future<Item = Gateway, Error = Error>>;
type ClientWith<S> = With<Client<S>, serde_json::Value, fn(serde_json::Value) -> Result<OwnedMessage, Error>, Result<OwnedMessage, Error>>;

/// represents an asyncronous connection to a discord gateway
/// 
/// does not handle reconnects automatically, but it should disconnect gracefully.
pub struct Gateway {
    ws: ClientWith<::websocket::client::async::TlsStream<::tokio_core::net::TcpStream>>,
    timer: Interval,
    last_seq: u64,
    heartbeat_buffer: Option< serde_json::Value>,
    should_close: bool,
    recieved_ack: bool,
}

impl Gateway {
    /// creates a future that will resolve to a discord gateway connection
    pub fn new(get_gateway_url: &str, handle: &reactor::Handle) -> Result<GatewayNew, Error> {
        let ws_handle = handle.clone();
        let api_client = ClientBuilder::new().build(handle)?;

        let wsf = api_client.get(get_gateway_url)
                        .send()
                        .map_err(|e| Error::from(e))
                        .and_then(|mut resp| {
                            resp.json::< serde_json::Value>().from_err()
                        }).and_then(move |url| {
                            let url = url["url"].as_str().unwrap();
                            WSClientBuilder::new(&url).map(|client| {
                                client.async_connect_secure(None, &ws_handle)
                            }).map_err(|e| Error::from(e)).into_future()
                        }).and_then(|x| x.map_err(|e| Error::from(e)))
                        .and_then(|(c, _)| {
                            c.into_future().map_err(|e| Error::from(e.0))
                        }).and_then(|c| {
                            let mut interval = 0;

                            if let Some(msg) = c.0.map(|c| recv_json(c, |value| {
                                GatewayEvent::decode(value)
                            })) {
                                match msg {
                                    Ok(GatewayEvent::Hello(beat)) => {
                                        debug!("connected to discord w/ heartbeat {}ms.", 
                                            beat
                                        );
                                        interval = beat;
                                    },
                                    _ => { return Err(Error::Other("did not recieve hello event!")); }
                                }
                            }


                            Ok(Gateway::from_client(c.1, interval))
                        });

        Ok(Box::new(wsf))
    }

    /// creates a gateway from an existing secure websocket connection and a given heartbeat interval
    pub fn from_client(conn: Client<::websocket::client::async::TlsStream<::tokio_core::net::TcpStream>>, interval: u64) -> Self {
        fn to_websocket(v: serde_json::Value) -> Result<OwnedMessage, Error> {
            debug!("send: {:?}", v);
            let msg = serde_json::to_string(&v)?;
            Ok(OwnedMessage::Text(msg))
        }
        
        let ws = conn.with(to_websocket as fn(_) -> _);
        
        Gateway {
            ws,
            timer: Timer::default().interval(Duration::from_millis(interval)),
            last_seq: 0,
            heartbeat_buffer: None,
            should_close: false,
            recieved_ack: true,
        }
    }

    fn try_heartbeat(&mut self) -> Poll<(), Error> {
        if let Some(item) = self.heartbeat_buffer.take() {
            if let AsyncSink::NotReady(item) = self.ws.start_send(item)? {
                self.heartbeat_buffer = Some(item);

                self.ws.poll_complete()?;

                return Ok(Async::NotReady);
            }
        }
        
        Ok(Async::Ready(()))
    }
}

impl Stream for Gateway {
    type Item = GatewayEvent;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // return none if we need to close / reconnect
        if self.should_close {
            return Ok(Async::Ready(None));
        }

        self.try_heartbeat()?;

        // poll the timer, sending heartbeat as nessicary
        match self.timer.poll() {
            Ok(Async::Ready(Some(_))) => {
                if self.heartbeat_buffer.is_none() {
                    if !self.recieved_ack {
                        return Err(Error::Protocol("no ack between 2 heartbeats"));
                    }

                    debug!("sending heartbeat!");
                    self.heartbeat_buffer = Some( json! {{ "op": 1, "d": self.last_seq }} );

                    self.recieved_ack = false;
                } else {
                    // something went wrong, the user should try to reconnect
                    self.should_close = true;
                    return Err(Error::Protocol("couldn't send heartbeats"));
                }
            },
            _ => { debug!("no heartbeat") },
        }

        // then poll the websocket
        let msg = match self.ws.poll() {
            Ok(Async::Ready(m)) => m,
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(e) => return Err(e.into()),
        };

        if let Some(msg) = msg {
            let ev = recv_json(msg, |value| {
                GatewayEvent::decode(value)
            })?;

            debug!("recieved an event! {:?}", ev);

            match &ev {
                &GatewayEvent::HeartbeatAck => {
                    self.recieved_ack = true;
                },
                &GatewayEvent::Dispatch(s, _) => {
                    self.last_seq = s;                       
                },
                &GatewayEvent::InvalidateSession | &GatewayEvent::Reconnect => {
                    self.should_close = true;
                }
                _ => {},                    
            }

            Ok(Async::Ready(Some(ev)))
        } else {
            Ok(Async::Ready(None))
        }

    }

}

impl Sink for Gateway {
    type SinkItem = serde_json::Value;
    type SinkError = Error;

    fn start_send(&mut self, item: serde_json::Value) -> Result<AsyncSink<Self::SinkItem>, Self::SinkError> {
        // if we have a heartbeat, try to send that first
        self.try_heartbeat()?;

        // then send the value
        if self.heartbeat_buffer.is_none() {
            self.ws.start_send(item).map_err(|e| e.into())
        } else {
            Ok(AsyncSink::NotReady(item))
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {

        if self.heartbeat_buffer.is_some() {
            match self.try_heartbeat() {
                Ok(Async::Ready(m)) => m,
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Err(e) => return Err(e.into()),
            }
        }

        self.ws.poll_complete().map_err(|e| e.into())
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        match self.try_heartbeat() {
            Ok(Async::Ready(m)) => m,
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(e) => return Err(e.into()),
        }
        
        self.should_close = true;
        Ok(self.ws.close()?)
    }
}

// this should go away soon
#[doc(hidden)]
pub struct Connection {
    send: UnboundedSender< serde_json::Value>,
    recv: mpsc::Receiver< GatewayEvent>,
    handle: thread::JoinHandle<Result<(), Error>>,
}

impl Connection {
    fn new(base_url: &str) -> Result<Self, Error> {
        let (send, recv) = unbounded();
        let (send_event, recv_event) = mpsc::channel();
        let base_url = base_url.to_string();

        let handle = thread::spawn(move || {
            let mut core = reactor::Core::new()?;
            let ws_handle = core.handle();

            let wsf = Gateway::new(&base_url, &ws_handle)?.and_then(|c| {

                let (sink, stream) = c.split();
                let stream = stream.map_err(|e| Error::from(e)).for_each(|e| {
                    send_event.send(e)?;
                    Ok(())
                });

                Ok(sink.send_all(recv.map_err(|_| Error::Other("channel err"))).join(stream).map(|_| ()))
            }).flatten();

            core.run(wsf)
        });

        Ok( Connection {
            send,
            recv: recv_event,
            handle,
        })
    }

    /// creates a new connection to discord
    pub fn connect(base_url: &str, token: &str, shard_info: Option<[u8; 2]>) -> Result<Self, Error> {
        let conn = Connection::new(base_url)?;
        conn.send.unbounded_send( identify(token, shard_info) )?;

        Ok(conn)
    }

    /// attempts to reconnect to a connection
    pub fn reconnect(base_url: &str, token: &str, sess: &str, seq: u64) -> Result<Self, Error> {
        let conn = Connection::new(base_url)?;

        let reconnect = json! {{
            "token": token,
            "session_id": sess,
            "seq": seq,
        }};

        conn.send.unbounded_send( reconnect )?;
        Ok(conn)
    }

    pub fn join(self) -> Result<(), Error> {
        self.handle.join().unwrap()
    }

    pub fn send(&self, msg: serde_json::Value) -> Result<(), Error> {
        self.send.unbounded_send(msg).map_err(|e| e.into())
    }

    pub fn recv(&self) -> Result<GatewayEvent, mpsc::RecvError> {
        self.recv.recv()
    }

    pub fn try_recv(&self) -> Result<GatewayEvent, mpsc::TryRecvError> {
        self.recv.try_recv()
    }
}

fn identify(token: &str, shard_info: Option<[u8; 2]>) -> serde_json::Value {
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
	result
}

fn recv_json<F, T>(message: OwnedMessage, decode: F) -> Result<T, Error> where F: FnOnce(serde_json::Value) -> Result<T, Error> {
	use std::io::Read;

    let message: Vec<u8> = match message {
        OwnedMessage::Binary(buf) => {
            use std::io::Read;
			let mut payload_vec = Vec::new();
			flate2::read::ZlibDecoder::new(&buf[..]).read_to_end(&mut payload_vec)?;
            payload_vec
        },
        OwnedMessage::Text(buf) => {
            buf.into_bytes()
        },
        OwnedMessage::Close(data) => {
            let code = data.as_ref().map(|d| d.status_code);
            let reason = data.map(|d| d.reason).unwrap_or(String::new());

            return Err(Error::Closed(code, reason));
        },
        m => {
            let message: Message = m.into(); 
            return Err(Error::Closed(None, String::from_utf8_lossy(&message.payload).into_owned()));
        }
    };

    serde_json::from_reader(&message[..]).map_err(From::from).and_then(decode).map_err(|e| {
		warn!("Error decoding: {}", String::from_utf8_lossy(&message));
		e
	})
}
