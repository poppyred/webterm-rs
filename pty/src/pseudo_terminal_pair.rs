use std::os::fd::{OwnedFd, AsFd};
use tokio::process::Child;

use super::{sys, PsuedoTerminal, AllocateError, SpawnError};

/// Pair of connected pseudo terminals.
///
/// One end of the terminal is for the parent process (usually the terminal emulator),
/// and one is for the child process running in the terminal emulator.
pub struct PsuedoTerminalPair {
	parent_tty: OwnedFd,
	child_tty: OwnedFd,
}

impl PsuedoTerminalPair {
	/// Allocate a new pseudo terminal with file descriptors for the parent and child end of the terminal.
	pub(super) fn new() -> Result<Self, AllocateError> {
		let parent_tty = sys::allocate_pty()
			.map_err(AllocateError::Open)?;
		sys::grant_child_terminal_permissions(parent_tty.as_fd())
			.map_err(AllocateError::Grant)?;
		sys::unlock_child_terminal(parent_tty.as_fd())
			.map_err(AllocateError::Unlock)?;

		let mut path_buffer = [0; 512];
		let child_tty_path = sys::get_child_terminal_path(parent_tty.as_fd(), &mut path_buffer)
			.map_err(AllocateError::GetChildName)?;

		let child_tty = std::fs::OpenOptions::new()
			.read(true)
			.write(true)
			.open(child_tty_path)
			.map_err(AllocateError::OpenChild)?
			.into();

		Ok(Self {
			parent_tty,
			child_tty,
		})
	}

	/// Spawn a child process as the session leader of a new process group with the pseudo terminal as controlling terminal.
	///
	/// Also returns the parent side of the pseudo terminal as [`PsuedoTerminal`] object.
	pub async fn spawn(self, mut command: tokio::process::Command) -> Result<(PsuedoTerminal, Child), SpawnError> {
		let Self { parent_tty, child_tty } = self;
		let stdin = child_tty;
		let stdout = stdin.try_clone()
			.map_err(SpawnError::DuplicateStdio)?;
		let stderr = stdin.try_clone()
			.map_err(SpawnError::DuplicateStdio)?;
		command.stdin(stdin);
		command.stdout(stdout);
		command.stderr(stderr);
		unsafe {
			command.pre_exec(move || {
				sys::create_process_group()
					.map_err(SpawnError::CreateSession)
					.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
				#[allow(clippy::useless_conversion)] // Not useless on all platforms.
				sys::set_controlling_terminal_to_stdin()
					.map_err(SpawnError::SetControllingTerminal)
					.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
				Ok(())
			});
		};
		let child = command.spawn()
			.map_err(SpawnError::Spawn)?;
		let tty = PsuedoTerminal::new(parent_tty)?;
		Ok((tty, child))
	}
}
