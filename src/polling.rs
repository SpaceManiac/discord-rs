use super::{Result, Error};

use websocket::client::{Client, Sender, Receiver};
use websocket::dataframe::DataFrame;
use websocket::stream::WebSocketStream;
use websocket::message::Message as WsMessage;
use websocket::message::Type as MessageType;

use serde_json;
use serde_json::builder::ObjectBuilder;

use super::model::*;

pub struct Connection {
	client: Client<DataFrame, Sender<WebSocketStream>, Receiver<WebSocketStream>>,
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
		let mut client = response.begin();
		
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
		try!(client.send_message(&WsMessage::text(try!(serde_json::to_string(&map)))));
		
		Ok(Connection {
			client: client
		})
	}

	pub fn recv_message(&mut self) -> Result<Event> {
		use websocket::ws::receiver::Receiver;
		let message: WsMessage = try!(self.client.get_mut_reciever().recv_message());
		if message.opcode == MessageType::Close {
			Ok(Event::Closed(message.cd_status_code.unwrap_or(0xffff)))
		} else if message.opcode != MessageType::Text {
			println!("Unexpected message: {:?}", message);
			Ok(Event::Closed(0xfffe))
		} else {
			let json: serde_json::Value = try!(serde_json::from_reader(&message.payload[..]));
			Event::decode(json)
		}
	}

	#[allow(dead_code)]
	fn send_keepalive(&mut self) -> Result<()> {
		let map = ObjectBuilder::new()
			.insert("op", 3)
			.insert_object("d", |object| object
				.insert("idle_since", serde_json::Value::Null)
				.insert("game_id", serde_json::Value::Null)
			)
			.unwrap();
		self.client.send_message(&WsMessage::text(try!(serde_json::to_string(&map)))).map_err(From::from)
	}

	pub fn shutdown(&mut self) -> Result<()> {
		//let (mut s, mut r) = self.client.split();
		try!(self.client.get_mut_sender().get_mut().shutdown(::std::net::Shutdown::Both));
		try!(self.client.get_mut_reciever().get_mut().get_mut().shutdown(::std::net::Shutdown::Both));
		Ok(())
	}
}




