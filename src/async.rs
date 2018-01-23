//! the core connection to discord, the heart of the bot
//! handles both api requests and gateway logic

use websocket::ClientBuilder as WSClientBuilder;
use websocket::OwnedMessage;
use reqwest::unstable::async::ClientBuilder;
use tokio_core::reactor;
use tokio_timer::Timer;
use futures::{IntoFuture, Future, Stream, Sink};
use futures::sync::mpsc::{UnboundedSender, unbounded, UnboundedReceiver};

use serde_json;
use std::time::Duration;
use std::thread;
use std::sync::atomic::{AtomicIsize, Ordering};

use error::Error;
use model::GatewayEvent;

const GATEWAY_VERSION: u64 = 6;

pub struct Connection {
    send: UnboundedSender< serde_json::Value>,
    recv: UnboundedReceiver< GatewayEvent>,
    handle: thread::JoinHandle<Result<(), Error>>,
}

impl Connection {
    pub fn run(token: &str) -> Result<Self, Error> {
        
        let (send, recv) = unbounded();
        let (send_event, recv_event) = unbounded();

        let handle = thread::spawn(move || {
            let mut core = reactor::Core::new()?;
            let api_client = ClientBuilder::new().build(&core.handle())?;
            let ws_handle = core.handle();

            let seq = AtomicIsize::new(-1);
            let wsf = api_client.get("https://discordapp.com/api/gateway")
                        .send()
                        .map_err(|e| Error::from(e))
                        .and_then(|mut resp| {
                            resp.json::<model::Gateway>().from_err()
                        }).and_then(|gateway| {
                            WSClientBuilder::new(&gateway.url).map(|client| {
                                client.async_connect_secure(None, &ws_handle)
                            }).map_err(|e| Error::from(e)).into_future()
                        }).and_then(|x| x.map_err(|e| Error::from(e)))
                        .and_then(|(c, _)| {
                            c.into_future().map_err(|e| Error::from(e.0))
                        }).and_then(|c| {
                            let mut interval = 0;

                            if let Some(OwnedMessage::Text(msg)) = c.0 {
                                let hello: model::GatewayPayload = serde_json::from_str(&msg).unwrap();
                                match hello {
                                    model::GatewayPayload::GatewayMsg {
                                        op: 10,
                                        d: model::GatewayMsg::Hello { heartbeat_interval, _trace },
                                    } => {
                                        
                                        debug!("connected to discord @'{}' w/ heartbeat {}ms.", 
                                            _trace[0],
                                            heartbeat_interval
                                        );
                                        interval = heartbeat_interval;
                                    },
                                    e => { bail!("invalid event: {:?}", e) }
                                }
                            }

                            let timer = Timer::default().interval(Duration::from_millis(interval)).map(|_|{
                                model::GatewayPayload::Heartbeat {
                                    op: 1,
                                    d: seq.load(Ordering::Relaxed),
                                }
                            }).map_err(|_| ()).select(recv).map(|e| {
                                debug!("send: {:?}", e);
                                OwnedMessage::Text(serde_json::to_string(&e).unwrap())
                            }).map_err(|_| RuntimeError::Connection);

                            let (sink, stream) = c.1.split();
                            let stream = stream.map_err(|e| Error::from(e)).for_each(|e| {
                                if let OwnedMessage::Text(msg) = e {
                                    let ev: model::GatewayPayload = serde_json::from_str(&msg)?;
                                    match ev {
                                        model::GatewayPayload::GatewayMsg { op: 11, .. } => {
                                            debug!("gateway heartbeat response!");
                                        }
                                        _ => { debug!("recv: {:?}", ev); }
                                    }
                                    
                                } else {
                                    debug!("recv: {:?}", e);
                                }
                                Ok(())
                            });

                            Ok(sink.sink_map_err(|e| Error::from(e)).send_all(timer).join(stream).map(|_| ()))
                        }).flatten();

            core.run(wsf)
        });

        send.unbounded_send(model::GatewayPayload::default_ident(token))?;

        Ok(Connection {
            send, 
            handle,
        })
    }

    pub fn join(self) -> Result<(), Error> {
        self.handle.join().unwrap()
    }

    pub fn send(&self, msg: model::GatewayPayload) -> Result<(), Error> {
        self.send.unbounded_send(msg).map_err(|e| e.into())
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