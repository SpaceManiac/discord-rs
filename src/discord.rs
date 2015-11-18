extern crate hyper;
extern crate serde_json;

use std::collections::BTreeMap;
use serde_json::builder::ObjectBuilder;

mod error;
mod model;
pub use error::{Result, Error};
pub use model::*;

const API_BASE: &'static str = "https://discordapp.com/api";

/// Discord client interface.
pub struct Discord {
	client: hyper::Client,
	token: String,
}

fn check_status(response: hyper::Result<hyper::client::Response>) -> Result<hyper::client::Response> {
	let response = try!(response);
	if !response.status.is_success() {
		return Err(Error::Status(response.status))
	}
	Ok(response)
}

#[allow(unused_variables)]
impl Discord {
	pub fn new(email: &str, password: &str) -> Result<Discord> {
		let mut map = BTreeMap::new();
		map.insert("email", email);
		map.insert("password", password);

		let client = hyper::Client::new();
		let response = try!(check_status(client.post(&format!("{}/auth/login", API_BASE))
			.header(hyper::header::ContentType::json())
			.body(&try!(serde_json::to_string(&map)))
			.send()));
		let mut json: BTreeMap<String, String> = try!(serde_json::from_reader(response));
		let token = match json.remove("token") {
			Some(token) => token,
			None => return Err(Error::Other("login: response missing key `token`"))
		};
		Ok(Discord {
			client: client,
			token: token,
		})
	}

	pub fn logout(self) -> Result<()> {
		let mut map = BTreeMap::new();
		map.insert("token", self.token);
		try!(check_status(self.client.post(&format!("{}/auth/logout", API_BASE))
			.header(hyper::header::ContentType::json())
			.body(&try!(serde_json::to_string(&map)))
			.send()));
		Ok(())
	}

	fn make_request(&self, request: hyper::client::RequestBuilder) -> Result<hyper::client::Response> {
		check_status(request
				.header(hyper::header::ContentType::json())
				.header(hyper::header::Authorization(self.token.clone()))
				.send())
	}

	pub fn create_channel(&self, server: &ServerId, name: &str, kind: ChannelType) -> Result<Channel> {
		let map = ObjectBuilder::new()
			.insert("name", name)
			.insert("type", kind.name())
			.unwrap();
		let response = try!(self.make_request(
			self.client.post(&format!("{}/guilds/{}/channels", API_BASE, server.0))
				.body(&try!(serde_json::to_string(&map)))));
		Channel::decode(try!(serde_json::from_reader(response)))
	}

	pub fn edit_channel(&self, channel: &ChannelId, name: &str) -> Result<Channel> {
		let map = ObjectBuilder::new()
			.insert("name", name)
			.unwrap();
		let response = try!(self.make_request(
			self.client.patch(&format!("{}/channels/{}", API_BASE, channel.0))
				.body(&try!(serde_json::to_string(&map)))));
		Channel::decode(try!(serde_json::from_reader(response)))
	}

	pub fn delete_channel(&self, channel: &ChannelId) -> Result<Channel> {
		let response = try!(self.make_request(
			self.client.delete(&format!("{}/channels/{}", API_BASE, channel.0))));
		Channel::decode(try!(serde_json::from_reader(response)))
	}

	/// Lasts 5 seconds.
	pub fn broadcast_typing(&self, channel: &ChannelId) -> Result<()> {
		try!(self.make_request(self.client.post(&format!("{}/channels/{}/typing", API_BASE, channel.0))));
		Ok(())
	}

	pub fn get_messages(&self, channel: &ChannelId, before: Option<&MessageId>, after: Option<&MessageId>, limit: Option<u64>) -> Result<Vec<Message>> {
		unimplemented!()
	}

	pub fn send_message(&self, channel: &ChannelId, text: &str, mentions: &[&UserId], nonce: &str, tts: bool) -> Result<Message> {
		let map = ObjectBuilder::new()
			.insert("content", text)
			.insert_array("mentions", |mut array| {
				for mention in mentions {
					array = array.push(&mention.0);
				}
				array
			})
			.insert("nonce", nonce)
			.insert("tts", tts)
			.unwrap();
		let response = try!(check_status(self.client.post(&format!("{}/channels/{}/messages", API_BASE, channel.0))
			.header(hyper::header::ContentType::json())
			.header(hyper::header::Authorization(self.token.clone()))
			.body(&try!(serde_json::to_string(&map)))
			.send()));
		Message::decode(try!(serde_json::from_reader(response)))
	}

	pub fn edit_message(&self, channel: &ChannelId, message: &MessageId, text: &str, mentions: &[&UserId]) -> Result<Message> {
		unimplemented!()
	}

	pub fn delete_message(&self, channel: &ChannelId, message: &MessageId) -> Result<()> {
		unimplemented!()
	}

	pub fn ack_message(&self, channel: &ChannelId, message: &MessageId) -> Result<()> {
		unimplemented!()
	}
}

