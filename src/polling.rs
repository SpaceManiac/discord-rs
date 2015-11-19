use super::{Result, Error};

use websocket::client::{Client, Sender, Receiver};
use websocket::dataframe::DataFrame;
use websocket::stream::WebSocketStream;
use websocket::message::Message as WsMessage;

use serde_json;
use serde_json::builder::ObjectBuilder;

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
		let request = try!(Client::connect(url));
		let response = try!(request.send());
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
		
		println!("Waiting on messages...");
		for message in client.incoming_messages() {
			let message: WsMessage = try!(message);
			match ::std::str::from_utf8(&message.payload) {
				Ok(text) => println!("{}", text),
				Err(e) => println!("NOT UTF-8")
			}
		}
		
		Ok(Connection {
			client: client
		})
	}
}

