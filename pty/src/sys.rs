use std::ffi::{c_int, OsStr, CStr, c_char};
use std::os::fd::{BorrowedFd, AsRawFd, OwnedFd, FromRawFd};
use std::os::unix::prelude::OsStrExt;

/// Allocate a new pseudo terminal.
pub fn allocate_pty() -> std::io::Result<OwnedFd> {
	unsafe {
		let parent_fd = check_return(libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK | libc::O_CLOEXEC))?;
		Ok(OwnedFd::from_raw_fd(parent_fd))
	}
}

/// Grant permissions on the child terminal device to the calling process.
pub fn grant_child_terminal_permissions(parent_tty: BorrowedFd<'_>) -> std::io::Result<()> {
	unsafe {
		check_return(libc::grantpt(parent_tty.as_raw_fd()))?;
		Ok(())
	}
}

/// Unlock the child terminal.
pub fn unlock_child_terminal(parent_tty: BorrowedFd<'_>) -> std::io::Result<()> {
	unsafe {
		check_return(libc::unlockpt(parent_tty.as_raw_fd()))?;
		Ok(())
	}
}

/// Get the path of the child terminal device.
pub fn get_child_terminal_path<'a>(parent_tty: BorrowedFd<'_>, buffer: &'a mut [c_char]) -> std::io::Result<&'a std::path::Path> {
	unsafe {
		check_return(libc::ptsname_r(parent_tty.as_raw_fd(), buffer.as_mut_ptr(), buffer.len()))?;
		let name = OsStr::from_bytes(CStr::from_ptr(buffer.as_ptr()).to_bytes());
		let name = std::path::Path::new(name);
		Ok(name)
	}
}

/// Read from a file descriptor.
pub fn read(file: BorrowedFd<'_>, buffer: &mut [u8]) -> std::io::Result<usize> {
	unsafe {
		check_return_size(libc::read(file.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()))
	}
}

/// Write to a file descriptor.
pub fn write(file: BorrowedFd<'_>, buffer: &[u8]) -> std::io::Result<usize> {
	unsafe {
		check_return_size(libc::write(file.as_raw_fd(), buffer.as_ptr().cast(), buffer.len()))
	}
}

/// Resize a pseudo terminal using an ioctl.
pub fn resize_pty(file: BorrowedFd<'_>, width: u32, height: u32) -> std::io::Result<()> {
	unsafe {
		let winsz = libc::winsize {
			ws_col: width.try_into().unwrap_or(u16::MAX),
			ws_row: height.try_into().unwrap_or(u16::MAX),
			ws_xpixel: 0,
			ws_ypixel: 0,
		};
		#[allow(clippy::useless_conversion)] // Not useless on all platforms.
		check_return(libc::ioctl(file.as_raw_fd(), libc::TIOCSWINSZ.into(), &winsz))?;
		Ok(())
	}
}

/// Create a new process group of which the calling process will be the session leader.
pub fn create_process_group() -> std::io::Result<()> {
	unsafe {
		check_return(libc::setsid())?;
		Ok(())
	}
}

/// Set the controlling terminal of the process group.
pub fn set_controlling_terminal_to_stdin() -> std::io::Result<()> {
	unsafe {
		#[allow(clippy::useless_conversion)] // Not useless on all platforms.
		check_return(libc::ioctl(0, libc::TIOCSCTTY.into(), 0))?;
		Ok(())
	}
}

/// Check the return value of a libc function that returns a `c_int`.
fn check_return(value: c_int) -> std::io::Result<c_int> {
	if value >= 0 {
		Ok(value)
	} else {
		Err(std::io::Error::last_os_error())
	}
}

/// Check the return value of a libc function that returns an `isize`.
fn check_return_size(value: isize) -> std::io::Result<usize> {
	if value < 0 {
		Err(std::io::Error::last_os_error())
	} else {
		Ok(value as usize)
	}
}
