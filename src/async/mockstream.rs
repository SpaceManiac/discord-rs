use super::imports::*;

use std::sync::Mutex;
use std::sync::Arc;

use futures::sync::mpsc::{Receiver, Sender, channel, unbounded};
use futures::{Stream, Sink, StartSend};

#[derive(Debug)]
pub struct MockStream<Item, Error>
{
    pub channel: Receiver<Result<Option<Item>, Error>>
}

pub struct MockStreamController<Item, Error>
{
    pub channel: Sender<Result<Option<Item>, Error>>
}

impl<Item, Error> MockStreamController<Item, Error> {
    pub fn send_item(&mut self, item: Item) -> () {
        self.channel.start_send(Ok(Some(item)));
    }

    pub fn send_err(&mut self, err: Error) -> () {
        self.channel.start_send(Err(err));
    }

    pub fn send_eof(&mut self) -> () {
        self.channel.start_send(Ok(None));
    }
}

impl<Item, Error> Stream for MockStream<Item, Error> {
    type Item = Item;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.channel.poll() {
            Err(_) => {
                // channel closed, we'll just indicate EOF
                Ok(Async::Ready(None))
            }
            Ok(Async::Ready(Some(Ok(result)))) => Ok(Async::Ready(result)),
            Ok(Async::Ready(Some(Err(e)))) => Err(e),
            Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
            Ok(Async::NotReady) => Ok(Async::NotReady)
        }
    }
}

impl<Item, Error> MockStream<Item, Error> {
    pub fn new(size: usize) -> (MockStreamController<Item,Error>, Self) {
        let (tx, rx) = channel(size);

        (MockStreamController { channel: tx }, MockStream { channel: rx })
    }
}

#[derive(Debug)]
pub enum MockSinkState<E> {
    Flushed,
    NotFlushed,
    Full,
    Err(E),
    Closed
}

#[derive(Debug)]
pub struct MockSink<Item, Error> {
    pub state: Arc<Mutex<MockSinkState<Error>>>,
    pub channel: Sender<Item>
}

impl<Item, Error> MockSink<Item, Error> {
    pub fn new(size: usize) -> (MockSink<Item, Error>, Receiver<Item>, Arc<Mutex<MockSinkState<Error>>>) {
        let state = Arc::new(Mutex::new(MockSinkState::Flushed));
        let (tx, rx) = channel(size);

        (MockSink { state: state.clone(), channel: tx }, rx, state)
    }
}

fn io_error<E: From<::std::io::Error>>(description: &'static str) -> E {
    use std::io::{Error, ErrorKind};
    From::from(Error::new(ErrorKind::Other, description))
}

fn take_error<E: From<::std::io::Error>>(error: &mut E) -> E {
    ::std::mem::replace(error, io_error("error taken twice"))
}

impl<Item,Error: From<::std::io::Error>> Sink for MockSink<Item, Error> {
    type SinkItem = Item;
    type SinkError = Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        let mut state = self.state.lock().unwrap();

        match *state {
            MockSinkState::Full => return Ok(AsyncSink::NotReady(item)),
            MockSinkState::Err(ref mut e) => return Err(take_error(e)),
            MockSinkState::Closed => return Err(io_error("already closed")),
            _ => {}
        }

        match self.channel.start_send(item) {
            Ok(AsyncSink::Ready) => {
                *state = MockSinkState::NotFlushed;
                Ok(AsyncSink::Ready)
            }
            Ok(AsyncSink::NotReady(item)) => {
                Ok(AsyncSink::NotReady(item))
            }
            Err(e) => Err(io_error("channel error"))
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        match *self.state.lock().unwrap() {
            MockSinkState::Flushed         => return Ok(Async::Ready(())),
            MockSinkState::Err(ref mut e)  => return Err(take_error(e)),
            MockSinkState::Closed          => return Err(io_error("already closed")),
            _                              => return Ok(Async::NotReady)
        }
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        (*self.state.lock().unwrap()) = MockSinkState::Closed;
        Ok(Async::Ready(()))
    }
}

pub struct Combined<Stream, Sink> {
    stream: Stream,
    sink: Sink
}

impl<Stream, Sink> Combined<Stream, Sink> {
    pub fn new(stream: Stream, sink: Sink) -> Self {
        Combined { stream: stream, sink: sink }
    }
}

impl<TStream: Stream, TSink> Stream for Combined<TStream, TSink> {
    type Item = TStream::Item;
    type Error = TStream::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.stream.poll()
    }
}

impl<TStream, TSink: Sink> Sink for Combined<TStream, TSink> {
    type SinkItem = TSink::SinkItem;
    type SinkError = TSink::SinkError;

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.sink.poll_complete()
    }

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.sink.start_send(item)
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.sink.close()
    }
}
