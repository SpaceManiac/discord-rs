use super::imports::*;
use time::precise_time_ns;
use websocket::{OwnedMessage, WebSocketError};
use std::io::{Error, ErrorKind};
use std::str::FromStr;

pub struct FaultInjecting<T: Stream<Error=WebSocketError> + Sink<SinkError=WebSocketError>> {
    upstream: T,
    fault_mode: ErrorType,
    next_fault: u64
}

#[derive(Debug, Clone, Copy)]
struct FaultConfig {
    fault_interval: u64,
    fault_mode: ErrorType
}

#[derive(Debug, Clone, Copy)]
enum ErrorType {
    None,        // don't inject an error
    CloseEvent,  // Closed message delivered
    StreamEnded, // None from stream
    StreamErr,   // Err from stream
    SinkErr      // Err from sink
}

impl FromStr for ErrorType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" => Ok(ErrorType::None),
            "CloseEvent" => Ok(ErrorType::CloseEvent),
            "StreamEnded" => Ok(ErrorType::StreamEnded),
            "StreamErr" => Ok(ErrorType::StreamErr),
            "SinkErr" => Ok(ErrorType::SinkErr),
            _ => Err(Error::new(ErrorKind::Other, format!("unknown error type {:?}", s)))
        }
    }
}

fn configure() -> FaultConfig {
    use std::env::var_os;

    let fault_interval =
        var_os("FAULT_INJECT_TIME_MS").and_then(|s|
            match s.into_string().unwrap().parse::<u64>() {
                Ok(e) => Some(e * 1000 * 1000),
                Err(err) => {
                    println!("Warning: can't parse FAULT_INJECT_TIME_MS: {:?}", err);
                    None
                }
            }
        ).unwrap_or(u64::max_value());

    let fault_mode =
        var_os("FAULT_INJECT_MODE").and_then(|s|
            match s.into_string().unwrap().parse::<ErrorType>() {
                Ok(e) => Some(e),
                Err(err) => {
                    println!("Warning: can't parse FAULT_INJECT_MODE: {:?}", err);
                    None
                }
            }
        ).unwrap_or(ErrorType::None);

    FaultConfig {
        fault_mode: fault_mode,
        fault_interval: fault_interval
    }
}

impl<T: Stream<Error=WebSocketError> + Sink<SinkError=WebSocketError>> FaultInjecting<T> {
    pub fn new(upstream: T) -> Self {
        let config = configure();

        println!("Configuring fault injection: {:?}", config);

        FaultInjecting {
            upstream: upstream,
            next_fault: config.fault_interval.saturating_add(precise_time_ns()),
            fault_mode: config.fault_mode
        }
    }

    fn fault_due(&mut self) -> ErrorType {
        if self.next_fault > precise_time_ns() {
            return ErrorType::None;
        }

        println!("!!! Fault due: {:?}", self.fault_mode);

        return self.fault_mode;
    }

    fn sink_fault(&mut self) -> Result<(), WebSocketError> {
        match self.fault_due() {
            ErrorType::SinkErr =>
                Err(WebSocketError::from(Error::new(ErrorKind::Other, "¯\\_(ツ)_/¯"))),
            _ => Ok(())
        }
    }
}

impl<T: Stream<Item=OwnedMessage, Error=WebSocketError> + Sink<SinkError=WebSocketError>> Stream for FaultInjecting<T> {
    type Item = OwnedMessage;
    type Error = WebSocketError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.fault_due() {
            ErrorType::CloseEvent =>
                return Ok(Async::Ready(Some(OwnedMessage::Close(None)))),
            ErrorType::StreamEnded =>
                return Ok(Async::Ready(None)),
            ErrorType::StreamErr =>
                return Err(WebSocketError::from(Error::new(ErrorKind::Other, "¯\\_(ツ)_/¯"))),
            _ => {}
        }

        self.upstream.poll()
    }
}

impl<T: Stream<Error=WebSocketError> + Sink<SinkError=WebSocketError>> Sink for FaultInjecting<T> {
    type SinkItem = T::SinkItem;
    type SinkError = WebSocketError;

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.sink_fault()?;

        self.upstream.poll_complete()
    }

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.sink_fault()?;

        self.upstream.start_send(item)
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.sink_fault()?;

        self.upstream.close()
    }
}