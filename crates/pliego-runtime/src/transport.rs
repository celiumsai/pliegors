// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::{Display, Formatter};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::time::{Instant, Sleep, sleep};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransportLimits {
    pub max_connections: usize,
    pub http1_header_read_timeout_ms: u64,
    pub read_idle_timeout_ms: u64,
    pub write_idle_timeout_ms: u64,
    pub http2_max_concurrent_streams: u32,
    pub http2_initial_stream_window_bytes: u32,
    pub http2_initial_connection_window_bytes: u32,
    pub http2_max_send_buffer_bytes: usize,
}

impl Default for TransportLimits {
    fn default() -> Self {
        Self {
            max_connections: 2_048,
            http1_header_read_timeout_ms: 10_000,
            read_idle_timeout_ms: 30_000,
            write_idle_timeout_ms: 30_000,
            http2_max_concurrent_streams: 128,
            http2_initial_stream_window_bytes: 256 * 1_024,
            http2_initial_connection_window_bytes: 1_024 * 1_024,
            http2_max_send_buffer_bytes: 64 * 1_024,
        }
    }
}

impl TransportLimits {
    pub fn validate(&self) -> Result<(), TransportLimitError> {
        validate("max_connections", self.max_connections as u64, 1, 65_536)?;
        validate(
            "http1_header_read_timeout_ms",
            self.http1_header_read_timeout_ms,
            1,
            60_000,
        )?;
        validate(
            "read_idle_timeout_ms",
            self.read_idle_timeout_ms,
            1,
            60 * 60 * 1_000,
        )?;
        validate(
            "write_idle_timeout_ms",
            self.write_idle_timeout_ms,
            1,
            60 * 60 * 1_000,
        )?;
        validate(
            "http2_max_concurrent_streams",
            self.http2_max_concurrent_streams as u64,
            1,
            65_536,
        )?;
        validate(
            "http2_initial_stream_window_bytes",
            self.http2_initial_stream_window_bytes as u64,
            16 * 1_024,
            i32::MAX as u64,
        )?;
        validate(
            "http2_initial_connection_window_bytes",
            self.http2_initial_connection_window_bytes as u64,
            16 * 1_024,
            i32::MAX as u64,
        )?;
        validate(
            "http2_max_send_buffer_bytes",
            self.http2_max_send_buffer_bytes as u64,
            1_024,
            u32::MAX as u64,
        )?;
        Ok(())
    }

    pub fn digest(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("TransportLimits serialization is infallible");
        let digest = Sha256::digest(bytes);
        encode_hex(&digest)
    }

    pub(crate) fn http1_header_read_timeout(&self) -> Duration {
        Duration::from_millis(self.http1_header_read_timeout_ms)
    }

    pub(crate) fn read_idle_timeout(&self) -> Duration {
        Duration::from_millis(self.read_idle_timeout_ms)
    }

    pub(crate) fn write_idle_timeout(&self) -> Duration {
        Duration::from_millis(self.write_idle_timeout_ms)
    }
}

fn validate(
    name: &'static str,
    value: u64,
    minimum: u64,
    maximum: u64,
) -> Result<(), TransportLimitError> {
    if value < minimum || value > maximum {
        Err(TransportLimitError::InvalidPolicy {
            name,
            value,
            minimum,
            maximum,
        })
    } else {
        Ok(())
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransportLimitError {
    InvalidPolicy {
        name: &'static str,
        value: u64,
        minimum: u64,
        maximum: u64,
    },
}

impl Display for TransportLimitError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPolicy {
                name,
                value,
                minimum,
                maximum,
            } => write!(
                formatter,
                "invalid {name} {value}; expected {minimum}..={maximum}"
            ),
        }
    }
}

impl std::error::Error for TransportLimitError {}

pub(crate) struct TimedIo {
    inner: TcpStream,
    read_timeout: Duration,
    write_timeout: Duration,
    read_timer: Pin<Box<Sleep>>,
    write_timer: Pin<Box<Sleep>>,
}

impl TimedIo {
    pub(crate) fn new(inner: TcpStream, limits: &TransportLimits) -> Self {
        let read_timeout = limits.read_idle_timeout();
        let write_timeout = limits.write_idle_timeout();
        Self {
            inner,
            read_timeout,
            write_timeout,
            read_timer: Box::pin(sleep(read_timeout)),
            write_timer: Box::pin(sleep(write_timeout)),
        }
    }

    fn reset_read_timer(&mut self) {
        self.read_timer
            .as_mut()
            .reset(Instant::now() + self.read_timeout);
    }

    fn reset_write_timer(&mut self) {
        self.write_timer
            .as_mut()
            .reset(Instant::now() + self.write_timeout);
    }

    fn poll_read_timeout(&mut self, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.read_timer.as_mut().poll(context).map(|_| {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "PliegoRS connection read idle timeout",
            ))
        })
    }

    fn poll_write_timeout(&mut self, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.write_timer.as_mut().poll(context).map(|_| {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "PliegoRS connection write idle timeout",
            ))
        })
    }
}

impl AsyncRead for TimedIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buffer.filled().len();
        match Pin::new(&mut self.inner).poll_read(context, buffer) {
            Poll::Ready(Ok(())) => {
                if buffer.filled().len() > before {
                    self.reset_read_timer();
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => self.poll_read_timeout(context),
        }
    }
}

impl AsyncWrite for TimedIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match Pin::new(&mut self.inner).poll_write(context, buffer) {
            Poll::Ready(Ok(written)) => {
                if written > 0 {
                    self.reset_write_timer();
                }
                Poll::Ready(Ok(written))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => match self.poll_write_timeout(context) {
                Poll::Ready(Ok(())) => Poll::Ready(Ok(0)),
                Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                Poll::Pending => Poll::Pending,
            },
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match Pin::new(&mut self.inner).poll_flush(context) {
            Poll::Ready(Ok(())) => {
                self.reset_write_timer();
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => self.poll_write_timeout(context),
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_transport_policy_is_valid_and_digest_is_stable() {
        let limits = TransportLimits::default();
        limits.validate().unwrap();
        assert_eq!(limits.digest(), limits.clone().digest());
        assert_eq!(limits.digest().len(), 64);
    }

    #[test]
    fn invalid_transport_policy_fails_closed() {
        let limits = TransportLimits {
            max_connections: 0,
            ..TransportLimits::default()
        };
        assert!(limits.validate().is_err());

        let limits = TransportLimits {
            http2_max_send_buffer_bytes: u32::MAX as usize + 1,
            ..TransportLimits::default()
        };
        assert!(limits.validate().is_err());
    }
}
