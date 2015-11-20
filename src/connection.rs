use super::{Result, Error};

use std::sync::mpsc;

use websocket::ws::sender::Sender as SenderTrait;
use websocket::client::{Client, Sender, Receiver};
use websocket::stream::WebSocketStream;
use websocket::message::{Message as WsMessage, Type as MessageType};

use serde_json;
use serde_json::builder::ObjectBuilder;

use super::model::*;

/// Websocket connection to the Discord servers.
pub struct Connection {
	keepalive_channel: mpsc::Sender<Status>,
	receiver: Receiver<WebSocketStream>,
	ready_event: Option<Event>,
}

impl Connection {
	pub fn new(url: &str, token: &str) -> Result<Connection> {
		// establish the websocket connection
		let url = match ::websocket::client::request::Url::parse(url) {
			Ok(url) => url,
			Err(_) => return Err(Error::Other("Invalid URL in Connection::new()"))
		};
		let response = try!(try!(Client::connect(url)).send());
		try!(response.validate());
		let (mut sender, mut receiver) = response.begin().split();
		
		// send the handshake
		let map = ObjectBuilder::new()
			.insert("op", 2)
			.insert_object("d", |object| object
				.insert("token", token)
				.insert_object("properties", |object| object
					.insert("$os", ::std::env::consts::OS)
					.insert("$browser", "Howl library for Rust")
					.insert("$device", "howl")
					.insert("$referring_domain", "")
					.insert("$referrer", "")
				)
				.insert("v", 3)
			)
			.unwrap();
		try!(sender.send_message(&WsMessage::text(try!(serde_json::to_string(&map)))));

		// read the Ready event
		let ready = try!(recv_message(&mut receiver));
		let heartbeat_interval = match &ready {
			&Event::Ready { heartbeat_interval, .. } => heartbeat_interval,
			_ => return Err(Error::Other("first packet was not a READY"))
		};

		// spawn the keepalive thread
		let (tx, rx) = mpsc::channel();
		::std::thread::spawn(move || keepalive(heartbeat_interval, sender, rx));

		// return the connection
		Ok(Connection {
			keepalive_channel: tx,
			receiver: receiver,
			ready_event: Some(ready),
		})
	}

	pub fn set_game_id(&mut self, game_id: Option<u64>) {
		let _ = self.keepalive_channel.send(Status::SetGameId(game_id));
	}

	pub fn recv_message(&mut self) -> Result<Event> {
		// clear the ready event
		if let Some(ready) = self.ready_event.take() {
			Ok(ready)
		} else {
			recv_message(&mut self.receiver)
		}
	}

	pub fn shutdown(&mut self) -> Result<()> {
		let _ = self.keepalive_channel.send(Status::Shutdown);
		try!(self.receiver.get_mut().get_mut().shutdown(::std::net::Shutdown::Both));
		Ok(())
	}
}

fn recv_message(receiver: &mut Receiver<WebSocketStream>) -> Result<Event> {
	use websocket::ws::receiver::Receiver;
	let message: WsMessage = try!(receiver.recv_message());
	if message.opcode == MessageType::Close {
		Ok(Event::Closed(message.cd_status_code.unwrap_or(0xffff)))
	} else if message.opcode != MessageType::Text {
		println!("Unexpected message: {:?}", message);
		Ok(Event::Closed(0xfffe))
	} else {
		let json: serde_json::Value = try!(serde_json::from_reader(&message.payload[..]));
		println!("<<< {:?} >>>", json);
		Event::decode(json)
	}
}

enum Status {
	Shutdown,
	SetGameId(Option<u64>),
}

fn keepalive(interval: u64, mut sender: Sender<WebSocketStream>, channel: mpsc::Receiver<Status>) {
	let mut countdown = interval;
	let mut game_id = None;
	loop {
		// TODO: this is not a precise timer, but it's good enough for now
		::std::thread::sleep_ms(1000);
		countdown = countdown.saturating_sub(1000);

		match channel.try_recv() {
			Ok(Status::Shutdown) => break,
			Ok(Status::SetGameId(id)) => { game_id = id; countdown = 0; },
			Err(mpsc::TryRecvError::Empty) => {},
			Err(mpsc::TryRecvError::Disconnected) => break
		}

		if countdown == 0 {
			countdown = interval;
			let map = ObjectBuilder::new()
				.insert("op", 3)
				.insert_object("d", |object| object
					.insert("idle_since", serde_json::Value::Null)
					.insert("game_id", game_id)
				)
				.unwrap();
			let json = match serde_json::to_string(&map) {
				Ok(json) => json,
				Err(e) => return println!("error jsoning ping: {:?}", e)
			};
			println!("Sending status ping: {}", json);
			match sender.send_message(&WsMessage::text(json)) {
				Ok(()) => {},
				Err(e) => return println!("error sending ping: {:?}", e)
			}
		}
	}
}

/// Server state tracking.
pub struct State;


