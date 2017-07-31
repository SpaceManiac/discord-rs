use async::imports::*;
use Discord;
use error::{Error, Result};
use futures::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};
use model::{ReadyEvent, Event};
use serde_json;


pub struct Connection {
    send: Sender<serde_json::Value>,
    recv: Option<Receiver<Result<Event>>>,
    conn_thread: JoinHandle<()>,
}

impl Connection {
    pub fn connect(discord: Discord, shard_info: Option<[u8; 2]>) -> Result<(Connection, ReadyEvent)> {
        let (send_tx, send_rx) = channel(1);
        let (recv_tx, recv_rx) = channel(1);

        let thread = thread::Builder::new()
            .name("Discord connection".into())
            .spawn(move|| { run_connection(discord, shard_info, send_rx, recv_tx) })?;

        let mut connection = Connection {
            send: send_tx,
            recv: Some(recv_rx),
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
        let maybe_item = match self.recv.take().unwrap().into_future().wait() {
            Ok((maybe_item, stream)) => {
                self.recv = Some(stream);

                maybe_item
            },
            Err((err, stream)) => {
                self.recv = Some(stream);

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