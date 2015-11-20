extern crate howl;

use howl::Discord;

fn main() {
	let discord = Discord::new("", "").expect("login failed");
	
	let test_zone = howl::ChannelId("".into());
	//println!("{:#?}", discord.send_message(&test_zone, "Hello from Rust", &[], "", false));
	discord.broadcast_typing(&test_zone).expect("broadcast typing failed");
	
	let mut connection = discord.connect().expect("connect failed");
	let closed;
	loop {
		let message = match connection.recv_message() {
			Ok(howl::Event::Closed(n)) => { closed = n; break },
			Ok(howl::Event::Ready { .. }) => { println!("Ready."); continue },
			other => other.expect("decode error"),
		};
		println!("    {:?}", message);
	}
	println!("closed upon: {}", closed);
	
	//discord.logout().expect("logout failed");
}
