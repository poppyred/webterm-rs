#[derive(Debug)]
pub enum AllocateError {
	Open(std::io::Error),
	Grant(std::io::Error),
	Unlock(std::io::Error),
	GetChildName(std::io::Error),
	OpenChild(std::io::Error),
}

#[derive(Debug)]
pub enum SpawnError {
	DuplicateStdio(std::io::Error),
	CreateSession(std::io::Error),
	SetControllingTerminal(std::io::Error),
	Spawn(std::io::Error),
	WrapAsyncFd(std::io::Error),
}

#[derive(Debug)]
pub struct ResizeError(pub std::io::Error);

impl std::error::Error for AllocateError {}
impl std::error::Error for SpawnError {}
impl std::error::Error for ResizeError {}

impl std::fmt::Display for AllocateError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			AllocateError::Open(e) => write!(f, "failed to open new pseudo terminal: {e}"),
			AllocateError::Grant(e) => write!(f, "failed to grant permissions on child terminal device: {e}"),
			AllocateError::Unlock(e) => write!(f, "failed to unlock child terminal device: {e}"),
			AllocateError::GetChildName(e) => write!(f, "failed to get name of child terminal device: {e}"),
			AllocateError::OpenChild(e) => write!(f, "failed to open child terminal device: {e}"),
		}
	}
}

impl std::fmt::Display for SpawnError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			SpawnError::DuplicateStdio(e) => write!(f, "failed to duplicate file descriptor for standard I/O stream: {e}"),
			SpawnError::CreateSession(e) => write!(f, "failed to create new process group: {e}"),
			SpawnError::SetControllingTerminal(e) => write!(f, "failed to set controlling terminal for new process group: {e}"),
			SpawnError::Spawn(e) => write!(f, "failed to spawn child process: {e}"),
			SpawnError::WrapAsyncFd(e) => write!(f, "failed to wrap pseudo terminal file descriptor for use with tokio: {e}"),
		}
	}
}

impl std::fmt::Display for ResizeError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let Self(e) = self;
		write!(f, "failed to resize terminal device: {e}")
	}
}
