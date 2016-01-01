extern crate discord;

use discord::Discord;
use discord::model::Event;
use std::env;

fn main() {
	// Log in to Discord using the email and password in the environment
	let discord = Discord::new(
		&env::var("DISCORD_EMAIL").expect("DISCORD_EMAIL"),
		&env::var("DISCORD_PASSWORD").expect("DISCORD_PASSWORD")
	).expect("login failed");

	// Establish and use a websocket connection
	let (mut connection, _) = discord.connect().expect("connect failed");
	println!("Ready.");
	loop {
		match connection.recv_event() {
			Ok(Event::Closed(n)) => {
				println!("Discord closed on us with status {}", n);
				break
			}
			Ok(Event::MessageCreate(message)) => {
				println!("{} says: {}", message.author.name, message.content);
				if message.content == "!test" {
					let _ = discord.send_message(&message.channel_id, "This is a reply to the test.", "", false);
				} else if message.content == "!quit" {
					println!("Quitting.");
					break
				}
			}
			Ok(_) => {}
			Err(err) => println!("Receive error: {:?}", err)
		}
	}

	// Log out from the API
	discord.logout().expect("logout failed");
}
