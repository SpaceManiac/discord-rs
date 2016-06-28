extern crate discord;

use std::env;
use discord::{Discord, State};
use discord::model::{Event, Channel};

// A simple "set topic" - bot example.
// Use by issuing the command "!topic <topic text>" in PM or a visible text channel.

pub fn main() {
	// log in to the API
	let discord = Discord::from_bot_token(&env::var("DISCORD_TOKEN").expect("Bad DISCORD_TOKEN")).expect("Login failed");

	// establish websocket and voice connection
	let (mut connection, ready) = discord.connect().expect("connect failed");
	println!("[Ready] {} is serving {} servers", ready.user.username, ready.servers.len());
	let mut state = State::new(ready);

	// receive events forever
	loop {
		let event = match connection.recv_event() {
			Ok(event) => event,
			Err(err) => {
				println!("[Warning] Receive error: {:?}", err);
				if let discord::Error::WebSocket(..) = err {
					// Handle the websocket connection being dropped
					let (new_connection, ready) = discord.connect().expect("connect failed");
					connection = new_connection;
					state = State::new(ready);
					println!("[Ready] Reconnected successfully.");
				}
				if let discord::Error::Closed(..) = err {
					break
				}
				continue
			},
		};
		state.update(&event);

		match event {
			Event::MessageCreate(message) => {
				use std::ascii::AsciiExt;
				// safeguard: stop if the message is from us
				if message.author.id == state.user().id {
					continue
				}

				// reply to a command if there was one
				let mut split = message.content.split(" ");
				let first_word = split.next().unwrap_or("");
				let argument = split.next().unwrap_or("");

				if first_word.eq_ignore_ascii_case("!topic") {
            let ch_info = discord.get_channel(&message.channel_id);
            if ch_info.is_ok() {
                match ch_info.unwrap() {
                    Channel::Public(channel) => {
                        let _ = discord.edit_channel(&message.channel_id,
                                                      Some(&channel.name),
                                                      Some(channel.position),
                                                      Some(argument));
                    },
                    _ => {},
                }
            }
				}
			},
			_ => {}, // discard other events
		}
	}
}
