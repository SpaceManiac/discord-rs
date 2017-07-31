use async::imports::*;
use Discord;
use error::{Error, Result};
use futures::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};
use model::*;
use serde_json;

/// Websocket connection to the Discord servers.
pub struct Connection {
    send_ch: Sender<serde_json::Value>,
    recv_ch: Option<Receiver<Result<Event>>>,
    conn_thread: JoinHandle<()>,
}

impl Connection {
    /// Establish a connection to the Discord websocket servers.
    ///
    /// Returns both the `Connection` and the `ReadyEvent` which is always the
    /// first event received and contains initial state information.
    pub fn connect(discord: Discord, shard_info: Option<[u8; 2]>) -> Result<(Connection, ReadyEvent)> {
        let (send_tx, send_rx) = channel(1);
        let (recv_tx, recv_rx) = channel(1);

        let thread = thread::Builder::new()
            .name("Discord connection".into())
            .spawn(move|| { run_connection(discord, shard_info, send_rx, recv_tx) })?;

        let mut connection = Connection {
            send_ch: send_tx,
            recv_ch: Some(recv_rx),
            conn_thread: thread
        };

        let event = connection.recv_event()?;

        if let Event::Ready(readyev) = event {
            Ok((connection, readyev))
        } else {
            Err(Error::Protocol("Expected Ready event on connect"))
        }
    }

    pub fn recv_event(&mut self) -> Result<Event> {
        let maybe_item = match self.recv_ch.take().unwrap().into_future().wait() {
            Ok((maybe_item, stream)) => {
                self.recv_ch = Some(stream);

                maybe_item
            },
            Err((err, stream)) => {
                self.recv_ch = Some(stream);

                // This outer error just indicates the other end of the channel was dropped.
                // We'll handle it below.
                None
            }
        };

        match maybe_item {
            Some(thing) => thing,
            None => Err(Error::Other("Unexpected exit of connection stream"))
        }
    }

    fn send(&self, msg: serde_json::Value) -> Result<()> {
        match self.send_ch.clone().send(msg).wait() {
            Ok(stream) => {
                Ok(())
            }
            Err(_) => Err(Error::Other("Unexpected connection thread exit"))
        }
    }

    /// Change the game information that this client reports as playing.
    pub fn set_game(&self, game: Option<Game>) {
        self.set_presence(game, OnlineStatus::Online, false)
    }

    /// Set the client to be playing this game, with defaults used for any
    /// extended information.
    pub fn set_game_name(&self, name: String) {
        self.set_presence(Some(Game::playing(name)), OnlineStatus::Online, false);
    }

    /// Sets the active presence of the client, including game and/or status
    /// information.
    ///
    /// `afk` will help Discord determine where to send notifications.
    pub fn set_presence(&self, game: Option<Game>, status: OnlineStatus, afk: bool) {
        let status = match status {
            OnlineStatus::Offline => OnlineStatus::Invisible,
            other => other,
        };
        let game = match game {
            Some(Game {kind: GameType::Streaming, url: Some(url), name}) => json! {{ "type": GameType::Streaming, "url": url, "name": name }},
            Some(game) => json! {{ "name": game.name }},
            None => json!(null),
        };
        let msg = json! {{
			"op": 3,
			"d": {
				"afk": afk,
				"since": 0,
				"status": status,
				"game": game,
			}
		}};
        let _ = self.send(msg);
    }

/* TODO
    /// Get a handle to the voice connection for a server.
    ///
    /// Pass `None` to get the handle for group and one-on-one calls.
    #[cfg(feature="voice")]
    pub fn voice(&mut self, server_id: Option<ServerId>) -> &mut VoiceConnection {
        let Connection { ref mut voice_handles, user_id, ref keepalive_channel, .. } = *self;
        voice_handles.entry(server_id).or_insert_with(||
            VoiceConnection::__new(server_id, user_id, keepalive_channel.clone())
        )
    }

    /// Drop the voice connection for a server, forgetting all settings.
    ///
    /// Calling `.voice(server_id).disconnect()` will disconnect from voice but retain the mute
    /// and deaf status, audio source, and audio receiver.
    ///
    /// Pass `None` to drop the connection for group and one-on-one calls.
    #[cfg(feature="voice")]
    pub fn drop_voice(&mut self, server_id: Option<ServerId>) {
        self.voice_handles.remove(&server_id);
    }
*/

    /// Cleanly shut down the websocket connection. Optional.
    pub fn shutdown(&mut self) -> Result<()> {
        use std::mem::replace;

        // Swap out the channels with new ones, this will trigger an error on the connection thread
        let (tx, rx) = channel(1);
        replace(&mut self.recv_ch, Some(rx));

        let (tx, rx) = channel(1);
        replace(&mut self.send_ch, tx);

        Ok(())
    }


    /// Requests a download of online member lists.
    ///
    /// It is recommended to avoid calling this method until the online member list
    /// is actually needed, especially for large servers, in order to save bandwidth
    /// and memory.
    ///
    /// Can be used with `State::all_servers`.
    pub fn sync_servers(&self, servers: &[ServerId]) {
        let msg = json! {{
			"op": 12,
			"d": servers,
		}};
        let _ = self.send(msg);
    }

    /// Request a synchronize of active calls for the specified channels.
    ///
    /// Can be used with `State::all_private_channels`.
    pub fn sync_calls(&self, channels: &[ChannelId]) {
        for &channel in channels {
            let msg = json! {{
				"op": 13,
				"d": { "channel_id": channel }
			}};
            let _ = self.send(msg);
        }
    }

    /// Requests a download of all member information for large servers.
    ///
    /// The members lists are cleared on call, and then refilled as chunks are received. When
    /// `unknown_members()` returns 0, the download has completed.
    pub fn download_all_members(&mut self, state: &mut ::State) {
        if state.unknown_members() == 0 { return }
        let servers = state.__download_members();
        let msg = json! {{
			"op": 8,
			"d": {
				"guild_id": servers,
				"query": "",
				"limit": 0,
			}
		}};
        let _ = self.send(msg);
    }
}

fn run_connection(
    discord: Discord,
    shard_info: Option<[u8; 2]>,
    send_rx: Receiver<serde_json::Value>,
    recv_tx: Sender<Result<Event>>
) {
    match run_connection0(discord, shard_info, send_rx, recv_tx.clone()) {
        Ok(_) => {
            let _ = recv_tx.wait().send(Err(Error::Other("Unknown error")));
        },
        Err(e) => {
            let _ = recv_tx.wait().send(Err(e));
        }
    };
}

fn run_connection0(
    discord: Discord,
    shard_info: Option<[u8; 2]>,
    mut send_rx: Receiver<serde_json::Value>,
    mut recv_tx: Sender<Result<Event>>
) -> Result<()> {
    use async;
    use tokio_core::reactor::{Core, Handle};

    let send_rx = send_rx.map_err(|_| Error::Other("Synchronous Connection dropped"));
    let recv_tx = recv_tx.sink_map_err(|_| Error::Other("Synchronous Connection dropped"));

    let mut core = Core::new()?;

    let mut conn = async::Connection::connect(discord, shard_info, &core.handle());
    let (mut conn_tx, mut conn_rx) = conn.split();

    let conn_rx = conn_rx.map(|ev| Ok(ev));

    // When the synchronous Connection is dropped, send_rx will error and this task will complete.
    let mut send_task = conn_tx.send_all(send_rx).map(|_| ());
    // Likewise if somehow the connection stops reconnecting this will fail
    let mut recv_task = recv_tx.send_all(conn_rx).map(|_| ());

    // If this returns we'll make a best effort attempt to propagate the error.
    core.run(send_task.select(recv_task).map_err(|(e, _)| e))?;

    // If we made it this far we have no idea why we exited. run_connection() will report an unknown
    // error.
    Ok(())
}