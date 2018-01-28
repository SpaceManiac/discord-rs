//! the core connection to discord, the heart of the bot
//! handles both api requests and gateway logic

use websocket::ClientBuilder as WSClientBuilder;
use websocket::{ Message, OwnedMessage };
use reqwest::unstable::async::ClientBuilder;
use tokio_core::reactor;
use tokio_timer::Timer;
use futures::{IntoFuture, Future, Stream, Sink};
use futures::sync::mpsc::{UnboundedSender, unbounded};
use std::sync::mpsc;

use flate2;
use serde_json;
use std::time::Duration;
use std::thread;
use std::sync::atomic::{AtomicIsize, Ordering};

use error::Error;
use model::GatewayEvent;

const GATEWAY_VERSION: u64 = 6;

/// this connection does not handle any reconnects
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
            let api_client = ClientBuilder::new().build(&core.handle())?;
            let ws_handle = core.handle();

            let seq = AtomicIsize::new(-1);
            let wsf = api_client.get(&base_url)
                        .send()
                        .map_err(|e| Error::from(e))
                        .and_then(|mut resp| {
                            resp.json::< serde_json::Value>().from_err()
                        }).and_then(|url| {
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
                            

                            let timer = Timer::default().interval(Duration::from_millis(interval)).map(|_|{
                                json! {{ "op": 1, "d": seq.load(Ordering::Relaxed) }}
                            }).map_err(|_| ()).select(recv).map(|e| {
                                debug!("send: {:?}", e);
                                OwnedMessage::Text(serde_json::to_string(&e).unwrap())
                            }).map_err(|_| Error::Other("timer"));

                            let (sink, stream) = c.1.split();
                            let stream = stream.map_err(|e| Error::from(e)).for_each(|e| {
                                let ev = recv_json(e, |value| {
                                    GatewayEvent::decode(value)
                                })?;

                                debug!("recieved an event! {:?}", ev);

                                match ev {
                                    // deal with this later
                                    GatewayEvent::HeartbeatAck => {},
                                    GatewayEvent::Dispatch(s, e) => {
                                        seq.store(s as isize, Ordering::Relaxed);
                                        send_event.send(GatewayEvent::Dispatch(s, e))?;
                                    },
                                    e => {
                                        send_event.send(e)?;
                                    }
                                }
                                Ok(())
                            });

                            Ok(sink.sink_map_err(|e| Error::from(e)).send_all(timer).join(stream).map(|_| ()))
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
