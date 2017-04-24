extern crate discord;

use discord::Discord;
use std::env;

#[allow(deprecated)]
fn main() {
	// To see the token cache in action, try the following sequence of commands:
	// $ cargo run --example login_cache <email>
	// The login will fail because no password was specified.
	// $ cargo run --example login_cache <email> <password>
	// The login should succeed.
	// $ cargo run --example login_cache <email>
	// The login will still succeed, because the cached token will be used.

	let args: Vec<_> = env::args().collect();
	Discord::new_cache(
		"tokens.txt",
		args.get(1).expect("No email specified"),
		args.get(2).map(|x| &**x),
	).expect("Login failed");
	println!("Logged in successfully!");
}
