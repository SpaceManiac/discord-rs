//! the core connection to discord, the heart of the bot
//! handles both api requests and gateway logic

use websocket::ClientBuilder as WSClientBuilder;
use websocket::async::Client;
use websocket::{ Message, OwnedMessage };


use reqwest::unstable::async::ClientBuilder;
use tokio_core::reactor;
use tokio_timer::{ Timer, Interval };

use websocket::async::futures::task;
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

use futures::sync::mpsc::{UnboundedSender, unbounded, UnboundedReceiver};
use std::sync::mpsc;

use flate2;
use serde_json;
use std::time::Duration;
use std::thread;

use error::Error;
use model::GatewayEvent;

/// a future that will create a discord gateway connection.
pub type GatewayNew = Box<Future<Item = Gateway, Error = Error>>;
type ClientWith<S> = With<Client<S>, serde_json::Value, fn(serde_json::Value) -> Result<OwnedMessage, Error>, Result<OwnedMessage, Error>>;

/// represents an asyncronous connection to a discord gateway
/// 
/// does not handle reconnects automatically, but it should disconnect gracefully.
/// 
pub struct Gateway {
    ws: ClientWith<::websocket::client::async::TlsStream<::tokio_core::net::TcpStream>>,
    timer: Interval,
    last_seq: u64,
    heartbeat_buffer: Option< serde_json::Value>,
    should_close: bool,
    recieved_ack: bool,
    is_voice: bool,
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
                            Gateway::from_gateway(url, false, &ws_handle)
                        });

        Ok(Box::new(wsf))
    }

    /// creates a websocket connection from a known gateway endpoint
    pub fn from_gateway(gateway_url: &str, is_voice: bool, handle: &reactor::Handle) -> GatewayNew {
        let url = if is_voice {
            format!("wss://{}/?v=3", gateway_url)
        } else {
            gateway_url.to_string()
        };
        
        let wsf = WSClientBuilder::new(&url).map(|client| {
            client.async_connect_secure(None, handle)
        }).into_future().flatten().map_err(|e| Error::from(e))
        .and_then(|(c, _)| {
            c.into_future().map_err(|e| Error::from(e.0))
        }).and_then(move |c| {
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

            Ok(Gateway::from_client(c.1, interval, is_voice))    
        });

        Box::new(wsf)
    }

    /// creates a gateway from an existing secure websocket connection and a given heartbeat interval
    fn from_client(conn: Client<::websocket::client::async::TlsStream<::tokio_core::net::TcpStream>>, interval: u64, is_voice: bool) -> Self {
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
            is_voice,
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
                    let t = task::current();
                    t.notify();

                    self.recieved_ack = false;
                } else {
                    // something went wrong, the user should try to reconnect
                    self.should_close = true;
                    return Err(Error::Protocol("couldn't send heartbeats"));
                }
            },
            _ => {},
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

// type to restart the gateway after a reconnect / restart
type GatewayNewSender = UnboundedSender<(String, mpsc::Sender<Result<GatewayEvent, Error>>, 
                                         UnboundedReceiver< serde_json::Value>)>;

#[doc(hidden)]
pub struct ConnectionHandle {
    gateway: Option<GatewayNewSender>,
    send: Option<UnboundedSender< serde_json::Value>>,
    recv: Option<mpsc::Receiver<Result<GatewayEvent, Error>>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl ConnectionHandle {
    /// create a new thread for running gateway clients
    pub fn new() -> Result<Self, Error> {
        let (gateway, gateway_recv): (GatewayNewSender, _) = unbounded();
        
        let handle = thread::Builder::new().name("discord-gateway".into()).spawn(move || {
            let mut core = reactor::Core::new().expect("cannot create event loop");
            let handle = core.handle();

            // for each gateway connection
            let work = gateway_recv.for_each(|(gateway_str, tx, rx)| {
                let err_tx = tx.clone();
                
                let wsf = Gateway::new(&gateway_str, &handle)
                                 .into_future()
                                 .flatten()
                                 .map(|gateway| {
                                     let (sink, stream) = gateway.split();

                                     // send all the values in the stream
                                     let stream = stream.for_each(move |e| {
                                        tx.send(Ok(e))?;
                                        Ok(())
                                     });

                                     let sink = sink.send_all(rx.map_err(|_| Error::Other("channel err")));

                                     sink.join(stream)
                                 }).flatten().map_err(move |e| {
                                     debug!("stream error: {:?}", e);
                                     // if the stream errors, send them too
                                     let _ = err_tx.send(Err(e));
                                 }).map(|_| ());
                
                handle.spawn(wsf);

                Ok(())
            });

            // this thread should never exit unless we drop the gateway channel (shutdown)
            // or something went terribly wrong (panic)
            let _ = core.run(work);
        })?;

        Ok(ConnectionHandle {
            gateway: Some(gateway), 
            send: None,
            recv: None,
            handle: Some(handle),
        })
    }

    /// spawn a new connection to discord
    pub fn connect(&mut self, get_gateway_str: &str) -> Result<(), Error> {
        if self.send.is_some() {
            self.disconnect();
        }

        let (send, gateway_recv) = unbounded();
        let (gateway_send, recv) = mpsc::channel();

        self.send = Some(send);
        self.recv = Some(recv);

        self.gateway.as_ref().map(|g| {
            g.unbounded_send((get_gateway_str.to_string(), gateway_send, gateway_recv)).map_err(|_| {
                Error::Other("can't send new client")
            })
        }).unwrap_or(Err(Error::Other("no gateway thread")))
    }

    /// close the currently running connection
    pub fn disconnect(&mut self) {
        debug!("closing gateway");
        self.send.take();
        self.recv.take();
    }

    /// send
    pub fn send(&self, value: serde_json::Value) -> Result<(), Error> {
        self.send.as_ref().and_then(|s| {
            s.unbounded_send(value).ok()
        }).ok_or(Error::Other("no gateway"))
    }

    /// recv (blocking)
    pub fn recv(&self) -> Result<GatewayEvent, Error> {
        self.recv.as_ref().and_then(|r| {
            r.recv().ok()
        }).unwrap_or(Err(Error::Other("no gateway")))
    }

    /// recv (non-blocking)
    pub fn try_recv(&self) -> Result<Option<GatewayEvent>, Error> {
        use std::sync::mpsc::TryRecvError;
        
        self.recv.as_ref().map(|r| {
            match r.try_recv() {
                Ok(Ok(ev)) => Ok(Some(ev)),
                Ok(Err(e)) => Err(e),
                Err(TryRecvError::Empty) => Ok(None),
                _ => Err(Error::Other("disconnected")),
            }
        }).unwrap_or(Err(Error::Other("no gateway")))
    }
}

impl Drop for ConnectionHandle {
    fn drop(&mut self) {
        self.disconnect();
        self.gateway.take();
        self.handle.take().map(|h| h.join());
    }
}

fn recv_json<F, T>(message: OwnedMessage, decode: F) -> Result<T, Error> where F: FnOnce(serde_json::Value) -> Result<T, Error> {

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
