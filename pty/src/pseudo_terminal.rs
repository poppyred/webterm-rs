use futures::ready;
use std::os::fd::{AsFd, OwnedFd};
use tokio::io::unix::AsyncFd;

use super::{sys, AllocateError, PsuedoTerminalPair, ResizeError, SpawnError};

/// Asynchronous pseudo terminal for [`tokio`].
pub struct PsuedoTerminal {
    inner: AsyncFd<OwnedFd>,
}

impl PsuedoTerminal {
    /// Allocate a new pseudo terminal.
    ///
    /// This returns a [`PsuedoTerminalPair`] that can be used to spawn a child process and obtain a handle to the parent side of the pseudo terminal.
    pub fn allocate() -> Result<PsuedoTerminalPair, AllocateError> {
        PsuedoTerminalPair::new()
    }

    /// Wrap an owned file descriptor.
    pub(super) fn new(fd: OwnedFd) -> Result<Self, SpawnError> {
        Ok(Self {
            inner: AsyncFd::new(fd).map_err(SpawnError::WrapAsyncFd)?,
        })
    }

    /// Resize the pseudo-terminal.
    ///
    /// Should be called when the terminal emulator changes size.
    pub fn resize(&self, width: u32, height: u32) -> Result<(), ResizeError> {
        sys::resize_pty(self.inner.as_fd(), width, height).map_err(ResizeError)
    }
}

impl tokio::io::AsyncRead for PsuedoTerminal {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        loop {
            let mut guard = ready!(self.inner.poll_read_ready(cx))?;

            let unfilled = buf.initialize_unfilled();
            match guard.try_io(|inner| sys::read(inner.as_fd(), unfilled)) {
                Ok(Ok(len)) => {
                    buf.advance(len);
                    return std::task::Poll::Ready(Ok(()));
                }
                Ok(Err(err)) => return std::task::Poll::Ready(Err(err)),
                Err(_would_block) => continue,
            }
        }
    }
}

impl tokio::io::AsyncWrite for PsuedoTerminal {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        loop {
            let mut guard = ready!(self.inner.poll_write_ready(cx))?;

            match guard.try_io(|inner| sys::write(inner.as_fd(), buf)) {
                Ok(result) => return std::task::Poll::Ready(result),
                Err(_would_block) => continue,
            }
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}
