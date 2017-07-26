use super::imports::*;
use error::Error;
use std::time::Duration;
use tokio_timer::Timer;
use futures_cpupool;
use internal::PrivateDiscord;
use futures::sync::oneshot::{Sender, Receiver};
use model::{Event, GatewayEvent};
use serde_json;
use std::collections::VecDeque;

static GATEWAY_VERSION: u64 = 6;

use Discord;
pub fn test(h: &Handle, d: Discord) -> BoxFuture<SingleConnection, Error> {
    use std::sync::Arc;
    use std::sync::Mutex;

    let info = SessionInfo {
        discord: Arc::new(d),
        gateway_url: None,
        gateway_failures: 0,
        session_id: None,
        last_seq: 0,
        shard_info: None,
        keepalive_interval: 0,
        timer: ::tokio_timer::Timer::default()
    };

    let sih = Arc::new(Mutex::new(info));

    start_connect(h, sih)
}

/// This module handles initial connection negotiation, particularly handling session resumption
/// (where feasible), and decoding of GatewayEvents into Events.
pub struct SingleConnection {
    session_info: SessionInfoRef,
    upstream: Box<Serializedish<
        // unfortunately these are not inferred from our trait definition
        Item=GatewayEvent,
        Error=Error,
        SinkItem=serde_json::Value,
        SinkError=Error
    >>,
    pending_event: Option<Event>,
    // This queue is used to store things like pending keepalives in case the tx side
    // is stuffed up when we should have sent one
    internal_send_queue: VecDeque<serde_json::Value>,
    next_keepalive: BoxFuture<(), Error>
}

impl SingleConnection {
    fn schedule_keepalive(&mut self) -> Result<(), Error> {
        let guard = self.session_info.lock().unwrap();
        let timer = &guard.timer;

        self.next_keepalive =
            timer.sleep(Duration::from_millis(guard.keepalive_interval))
                 .map_err(Error::from)
                 .boxed();

        Ok(())
    }

    fn send_keepalive(&mut self) -> Result<(), Error> {
        let map = {
            let guard = self.session_info.lock().unwrap();

            json! {{
				"op": 1,
				"d": guard.last_seq
			}}
        };

        println!("Trying to send keepalive");
        match self.start_send(map) {
            Ok(AsyncSink::Ready) => {
                println!("Sent keepalive");
                self.schedule_keepalive()
            },
            // If the tx side is filled up, we'll leave the timer future in the ready state, and
            // we'll come back to send as soon as there's room in the send queue.
            Ok(AsyncSink::NotReady(_)) => {
                self.next_keepalive = future::ok(()).boxed();
                Ok(())
            },
            Err(e) => Err(e)
        }
    }
}

impl Sink for SingleConnection {
    type SinkItem=serde_json::Value;
    type SinkError=Error;

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        if self.next_keepalive.poll()? == Async::Ready(()) {
            self.send_keepalive()?;
        }

        self.upstream.poll_complete()
    }

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.upstream.start_send(item)
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.upstream.close()
    }
}

impl Stream for SingleConnection {
    type Item=Event;
    type Error=Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Poll the TX side always. This triggers keepalive behavior - both by implicitly arranging
        // for the task to be awoken when the timer expires, and also by actually sending the
        // keepalive message.
        self.poll_complete()?;

        if let Some(event) = ::std::mem::replace(&mut self.pending_event, None) {
            return Ok(Async::Ready(Some(event)))
        }

        // Since we'll swallow certain event types, it's important to loop until we either yield
        // something to the downstream consumer or get a NotReady from the upstream (thus
        // registering the task to be awoken when incoming data arrives from the network).
        loop {
            let gw_event: GatewayEvent = match self.upstream.poll() {
                Err(e) => return Err(e),
                Ok(Async::NotReady) => {
                    println!("gw not ready");
                    return Ok(Async::NotReady)
                },
                Ok(Async::Ready(None)) => return Ok(Async::Ready(None)),
                Ok(Async::Ready(Some(gw_event))) => gw_event
            };

            match gw_event {
                GatewayEvent::Hello(interval) => {
                    debug!("Mysterious late-game hello: {}", interval);
                },
                GatewayEvent::Dispatch(sequence, event) => {
                    self.session_info.lock().unwrap().last_seq = sequence;
                    //let _ = self.keepalive_channel.send(Status::Sequence(sequence));
                    #[cfg(feature = "voice")] {
                        if let Event::VoiceStateUpdate(server_id, ref voice_state) = event {
                            //self.voice(server_id).__update_state(voice_state);
                        }
                        if let Event::VoiceServerUpdate { server_id, channel_id: _, ref endpoint, ref token } = event {
                            //self.voice(server_id).__update_server(endpoint, token);
                        }
                    }
                    return Ok(Async::Ready(Some(event)));
                },
                GatewayEvent::Heartbeat(sequence) => {
                    debug!("Heartbeat received with seq {}", sequence);
                    self.session_info.lock().unwrap().last_seq = sequence;

                    // Arrange to a keepalive ASAP.
                    self.next_keepalive = future::ok(()).boxed();

                    // ... and now give that send a chance to run
                    self.poll_complete()?;
                },
                GatewayEvent::HeartbeatAck => {}
                GatewayEvent::Reconnect => {
                    println!("recon req");
                    // Treat this as a disconnect
                    return Ok(Async::Ready(None));
                }
                GatewayEvent::InvalidateSession => {
                    println!("invalidate");
                    // Also treat this as a disconnect, but clear the session ID first
                    self.session_info.lock().unwrap().session_id = None;

                    return Ok(Async::Ready(None));
                }
            };
        }
    }
}

enum ResumeResult {
    NoSession,
    Resumed(u64, Event)
}

pub fn start_connect(handle: &Handle, session_info: SessionInfoRef)
    -> BoxFuture<SingleConnection, Error>
{
    println!("!!! start_connect");
    let session_guard = session_info.lock().unwrap();

    // First things first - do we need to back off?
    // FIXME
    let backoff = if session_guard.gateway_failures > 0 { 1000 } else { 0 };
    let backoff = Duration::from_millis(backoff);

    let timer = session_guard.timer.clone();
    let sleep = timer.sleep(backoff).map_err(Error::from);

    let info_ref = session_info.clone();
    let handle_ref = handle.clone();
    // Now to start the actual chain of futures that will connect us.
    let future = sleep.and_then(move |_| {
        let info_guard = info_ref.lock().unwrap();
        if info_guard.gateway_failures > 0
            || info_guard.gateway_url.is_none() {
            // Spawn a thread to go take care of getting the gateway URL for us.
            use ::std::thread;

            let (tx, rx) = oneshot::channel();

            let discord_ref = info_guard.discord.clone();
            let shard_info = info_guard.shard_info.clone();
            match thread::Builder::new()
                .name("Discord gateway lookup".into())
                .spawn(move|| {
                    println!("!!! gateway lookup");
                    tx.send(discord_ref.__get_gateway(shard_info));
                }) {
                Err(e) => return future::err(Error::from(e)).boxed(),
                Ok(_) => {}
            }

            let info_ref_clone = info_ref.clone();
            // outer errors indicate the tx channel was dropped
            rx.map_err(|_| Error::Other("Unexpected thread death"))
              .and_then(|result| result)
              .and_then(move |gateway_url| {
                  let gateway_url = format!("{}?v={}", gateway_url, GATEWAY_VERSION);

                  info_ref_clone.lock().unwrap().gateway_url = Some(gateway_url.clone());

                  Ok(gateway_url)
              })
              .box_via_err(&handle_ref, Error::Other("Unexpected error"))
        } else {
            let gateway_url = info_ref.lock().unwrap().gateway_url.clone().unwrap();
            future::ok(gateway_url).boxed()
        }
    }).box_via_err(&handle, Error::Other("Unexpected error"));

    let info_ref = session_info.clone();
    let handle_ref = handle.clone();
    let future = future.and_then(move |gateway_url: String| {
        use websocket::ClientBuilder;

        match ClientBuilder::new(&gateway_url) {
            Err(e) => future::err(Error::from(e)).boxed(),
            Ok(builder) => {
                builder.async_connect(None, &handle_ref).map_err(Error::from)
                    .box_via_err(&handle_ref, Error::Other("Connect task died"))
            },
        }
    }).box_via_err(&handle, Error::Other("Unexpected error"));

    let future = future.map(|(client, _headers)| {
          // We're connected, let's set up our client wrappers first
          use super::serializer::Serializer;
          use super::pingfilter::PingFilter;

          Serializer::new(PingFilter::new(client))
      })
      .and_then(|client| {
          // now receive the hello. Note that into_future will return the original client
          // on failure, but we don't need this behavior (because we'll be dropping the connection
          // anyway) so we map_err it away.
          client.into_future().map_err(|(err, _stream)| err)
      })
      .and_then(move |(item, client)| {
          match item {
              None => return Err(Error::Closed(None, "connection closed before hello".into())),
              Some(GatewayEvent::Hello(interval)) => {
                  info_ref.lock().unwrap().keepalive_interval = interval;
                  return Ok(client);
              }
              Some(event) => {
                  debug!("Unexpected event: {:?}", event);
                  return Err(Error::Protocol("Expected Hello during handshake"))
              }
          }
      });

    let info_ref = session_info.clone();
    // We received the hello, time for us to say hi.
    let future = future.and_then(move|client| session_handshake(client, info_ref));
    // And then time to set up keepalives
    let future = future.and_then(move|mut conn| {
        conn.schedule_keepalive()?;
        Ok(conn)
    });
    // Finally if there are any errors we'll make sure to capture them and increment the error
    // counter
    let info_ref = session_info.clone();
    let future = future.map_err(move |e| {
        // FIXME - distinguish between gateway failures and other
        info_ref.lock().unwrap().gateway_failures += 1;
        return e;
    });

    // and that's all there is to it! Box it up and return.
    future.box_via_err(&handle, Error::Other("Unexpected error"))
}

fn session_handshake<C>(conn: C, session_info: SessionInfoRef)
    -> BoxFuture<SingleConnection, Error>
    where C: Serializedish + Sized + Send + 'static
{
    let guard = session_info.lock().unwrap();

    let hello = match &guard.session_id {
        &Some(ref session_id) => {
            println!("resume");
            let resume = json! {{
                "op": 6,
                "d": {
                    "seq": guard.last_seq,
                    "session_id": session_id,
                    "token": String::from(session_info.lock().unwrap().token())
                }
            }};

            resume
        }
        &None => {
            println!("identify");
            let mut identify = json! {{
                "op": 2,
                "d": {
                    "token": String::from(guard.token()),
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

            if let Some(info) = guard.shard_info {
                identify["shard"] = json![[info[0], info[1]]];
            }

            identify
        }
    };

    let session_info_ref = session_info.clone();
    conn.send(hello).and_then(move |conn| {
        await_handshake_result(conn, session_info_ref)
    }).boxed()
}

fn await_handshake_result<C>(conn: C, session_info: SessionInfoRef)
                        -> BoxFuture<SingleConnection, Error>
    where C: Serializedish + Sized + Send + 'static
{
    conn.into_future()
        .map_err(|(e, _)| e)
        .and_then(|(message, conn)| {
            match message {
                None => return future::err(Error::Closed(None, "Unexpected close".into())).boxed(),
                Some(GatewayEvent::InvalidateSession) => {
                    {
                        println!("reject resume");
                        let mut guard = session_info.lock().unwrap();
                        if guard.session_id.is_none() {
                            return future::err(Error::Protocol("Invalid session during handshake. \
                            Double-check your token or consider waiting 5 seconds between starting shards."))
                                .boxed();
                        }
                        // TODO - delay 1-5s
                        guard.session_id = None;
                    }

                    return session_handshake(conn, session_info).boxed();
                },
                Some(GatewayEvent::Dispatch(seq, Event::Ready(event))) => {
                    let single_conn = SingleConnection {
                        session_info: session_info,
                        pending_event: Some(Event::Ready(event)),
                        upstream: Box::new(conn),
                        internal_send_queue: VecDeque::new(),
                        // this will be overwritten shortly, so just drop a placeholder for now
                        next_keepalive: future::ok(()).boxed()
                    };

                    return future::ok(single_conn).boxed();
                }
                other => {
                    debug!("Unexpected event: {:?}", other);
                    return future::err(Error::Protocol("Expected Ready during handshake")).boxed()
                }
            }
        })
        .boxed()
}