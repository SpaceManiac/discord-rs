extern crate discord;

use std::env;
use discord::{Discord, State};
use discord::voice::VoiceConnection;
use discord::model::Event;

// A simple DJ bot example.
// Use by issuing the command "!dj <youtube-link>" in PM or a visible text channel.
// The bot will join the voice channel of the person issuing the command.
// "!dj stop" will stop playing, and "!dj quit" will quit the voice channel.
// The bot will quit any voice channel it is the last user in.

pub fn main() {
	// log in to the API
	let args: Vec<_> = env::args().collect();
	let discord = Discord::new_cache(
		"tokens.txt",
		args.get(1).expect("No email specified"),
		args.get(2).map(|x| &**x),
	).expect("Login failed");

	// establish websocket and voice connection
	let (mut connection, ready) = discord.connect().expect("connect failed");
	println!("[Ready] {} is serving {} servers", ready.user.username, ready.servers.len());
	let mut voice = VoiceConnection::new(ready.user.id.clone());
	let mut state = State::new(ready);
	let mut current_channel = None;

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
				continue
			},
		};
		state.update(&event);
		voice.update(&event);

		match event {
			Event::Closed(n) => {
				println!("[Error] Connection closed with status: {}", n);
				break
			},
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

				if first_word.eq_ignore_ascii_case("!dj") {
					if argument.eq_ignore_ascii_case("stop") {
						voice.stop();
					} else if argument.eq_ignore_ascii_case("quit") {
						connection.voice_disconnect();
					} else {
						let output = (|| {
							for server in state.servers() {
								for vstate in &server.voice_states {
									if vstate.user_id == message.author.id {
										if let Some(ref chan) = vstate.channel_id {
											let stream = match discord::voice::open_ytdl_stream(&argument) {
												Ok(stream) => stream,
												Err(error) => return format!("Error: {}", error),
											};
											voice.stop();
											connection.voice_connect(&server.id, chan);
											voice.play(stream);
											return String::new()
										}
									}
								}
							}
							"You must be in a voice channel to DJ".into()
						})();
						if output.len() > 0 {
							warn(discord.send_message(&message.channel_id, &output, "", false));
						}
					}
				}
			}
			Event::VoiceStateUpdate(server_id, voice_state) => {
				if voice_state.user_id == state.user().id {
					current_channel = voice_state.channel_id.map(|c| (server_id, c));
				} else if let Some((ref cur_server, ref cur_channel)) = current_channel {
					// If we are in a voice channel, && someone on our server moves/hangs up,
					if *cur_server == server_id {
						// && our current voice channel is empty, disconnect from voice
						if let Some(srv) = state.servers().iter().find(|srv| srv.id == server_id) {
							if srv.voice_states.iter().filter(|vs| vs.channel_id.as_ref() == Some(cur_channel)).count() <= 1 {
								connection.voice_disconnect();
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
