extern crate discord;

use discord::{Discord, ChannelRef, State};
use discord::model::{Event, ChannelType};
use std::env;

fn main() {
	// Log in to Discord using the email and password in the environment
	let discord = Discord::new(
		&env::var("DISCORD_EMAIL").expect("DISCORD_EMAIL"),
		&env::var("DISCORD_PASSWORD").expect("DISCORD_PASSWORD")
	).expect("login failed");

	// Establish the websocket connection
	let (mut connection, ready) = discord.connect().expect("connect failed");
	let channel_count: usize = ready.servers.iter()
		.map(|srv| srv.channels.iter()
			.filter(|chan| chan.kind == ChannelType::Text)
			.count()
		).fold(0, |v, s| v + s);
	println!("[Ready] {} logging {} servers with {} text channels", ready.user.username, ready.servers.len(), channel_count);

	// Initialize the state
	let mut state = State::new(ready).expect("state creation failed");

	loop {
		// Receive an event and update the state with it
		let event = match connection.recv_event() {
			Ok(event) => event,
			Err(err) => {
				println!("[Warning] Receive error: {:?}", err);
				continue
			}
		};
		state.update(&event);

		// Log messages
		match event {
			Event::Closed(n) => {
				println!("[Error] Connection closed with status: {}", n);
				break
			},
			Event::MessageCreate(message) => {
				match state.find_channel(&message.channel_id) {
					Some(ChannelRef::Public(server, channel)) => {
						println!("[{} #{}] {}: {}", server.name, channel.name, message.author.name, message.content);
					}
					Some(ChannelRef::Private(channel)) => {
						if message.author.name == channel.recipient.name {
							println!("[Private] {}: {}", message.author.name, message.content);
						} else {
							println!("[Private] To {}: {}", channel.recipient.name, message.content);
						}
					}
					None => println!("[Unknown Channel] {}: {}", message.author.name, message.content),
				}
			}
			Event::Unknown(name, data) => {
				// log unknown event types for later study
				println!("[Unknown Event] {}: {:?}", name, data);
			}
			_ => {}, // discard other known events
		}
	}

	// Log out from the API
	discord.logout().expect("logout failed");
}
