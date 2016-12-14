extern crate discord;

use std::env;
use discord::{Discord, State};
use discord::model::Event;

// A simple DJ bot example.
// Use by issuing the command "!dj <youtube-link>" in PM or a visible text channel.
// The bot will join the voice channel of the person issuing the command.
// "!dj stop" will stop playing, and "!dj quit" will quit the voice channel.
// The bot will quit any voice channel it is the last user in.

pub fn main() {
	// Log in to Discord using a bot token from the environment
	let discord = Discord::from_bot_token(
		&env::var("DISCORD_TOKEN").expect("Expected token"),
	).expect("login failed");

	// establish websocket and voice connection
	let (mut connection, ready) = discord.connect().expect("connect failed");
	println!("[Ready] {} is serving {} servers", ready.user.username, ready.servers.len());
	let mut state = State::new(ready);
	connection.sync_calls(&state.all_private_channels());

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
				let mut split = message.content.split(' ');
				let first_word = split.next().unwrap_or("");
				let argument = split.next().unwrap_or("");

				if first_word.eq_ignore_ascii_case("!dj") {
					let vchan = state.find_voice_user(message.author.id);
					if argument.eq_ignore_ascii_case("stop") {
						vchan.map(|(sid, _)| connection.voice(sid).stop());
					} else if argument.eq_ignore_ascii_case("quit") {
						vchan.map(|(sid, _)| connection.drop_voice(sid));
					} else {
						let output = if let Some((server_id, channel_id)) = vchan {
							match discord::voice::open_ytdl_stream(argument) {
								Ok(stream) => {
									let voice = connection.voice(server_id);
									voice.set_deaf(true);
									voice.connect(channel_id);
									voice.play(stream);
									String::new()
								},
								Err(error) => format!("Error: {}", error),
							}
						} else {
							"You must be in a voice channel to DJ".to_owned()
						};
						if !output.is_empty() {
							warn(discord.send_message(message.channel_id, &output, "", false));
						}
					}
				}
			}
			Event::VoiceStateUpdate(server_id, _) => {
				// If someone moves/hangs up, and we are in a voice channel,
				if let Some(cur_channel) = connection.voice(server_id).current_channel() {
					// and our current voice channel is empty, disconnect from voice
					match server_id {
						Some(server_id) => if let Some(srv) = state.servers().iter().find(|srv| srv.id == server_id) {
							if srv.voice_states.iter().filter(|vs| vs.channel_id == Some(cur_channel)).count() <= 1 {
								connection.voice(Some(server_id)).disconnect();
							}
						},
						None => if let Some(call) = state.calls().get(&cur_channel) {
							if call.voice_states.len() <= 1 {
								connection.voice(server_id).disconnect();
							}
						}
					}
				}
			}
			_ => {}, // discard other events
		}
	}
}

fn warn<T, E: ::std::fmt::Debug>(result: Result<T, E>) {
	match result {
		Ok(_) => {},
		Err(err) => println!("[Warning] {:?}", err)
	}
}
