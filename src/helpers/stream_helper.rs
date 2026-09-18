use std::io::{self, Read, Write};
use std::fs::OpenOptions;
use std::path::Path;
use std::net::TcpStream;
use std::thread;
use std::time::Duration;
use url::Url;

#[cfg(windows)]
use log::{debug, warn};

#[cfg(windows)]
use std::os::windows::prelude::*;


/// AccessMode for opening a stream
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AccessMode {
    Read,
    Write,
    ReadWrite,
}

/// Wrapper type for different stream types
pub enum StreamWrapper {
    ReadOnly(Box<dyn Read + Send>),
    WriteOnly(Box<dyn Write + Send>),
    ReadWrite(Box<dyn ReadWrite + Send>),
}

/// A trait combining Read and Write
pub trait ReadWrite: Read + Write {}

// Implement ReadWrite for types that implement both Read and Write
impl<T: Read + Write + ?Sized> ReadWrite for T {}

impl StreamWrapper {
    /// Convert the wrapper to a readable stream
    pub fn as_reader(&mut self) -> io::Result<&mut dyn Read> {
        match self {
            StreamWrapper::ReadOnly(reader) => Ok(reader.as_mut()),
            StreamWrapper::ReadWrite(stream) => Ok(stream.as_mut()),
            StreamWrapper::WriteOnly(_) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied, 
                "Stream is write-only"
            )),
        }
    }
    
    /// Convert the wrapper to a writable stream
    pub fn as_writer(&mut self) -> io::Result<&mut dyn Write> {
        match self {
            StreamWrapper::WriteOnly(writer) => Ok(writer.as_mut()),
            StreamWrapper::ReadWrite(stream) => Ok(stream.as_mut()),
            StreamWrapper::ReadOnly(_) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied, 
                "Stream is read-only"
            )),
        }
    }
}

/// Open a stream from a source which can be a URL or a file path
/// 
/// # Arguments
/// 
/// * `source` - URL or file path to open
/// * `mode` - Access mode (Read, Write, or ReadWrite)
/// 
/// # Returns
/// 
/// A wrapped stream object that can be used for reading, writing, or both
pub fn open_stream(source: &str, mode: AccessMode) -> io::Result<StreamWrapper> {
    if let Ok(url) = Url::parse(source) {
        match url.scheme() {
            "tcp" => {
                let host = url.host_str().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Missing host"))?;
                let port = url.port().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Missing port"))?;
                let stream = TcpStream::connect((host, port))?;
                
                match mode {
                    AccessMode::Read => {
                        let reader = stream.try_clone()?;
                        Ok(StreamWrapper::ReadOnly(Box::new(reader)))
                    },
                    AccessMode::Write => {
                        let writer = stream.try_clone()?;
                        Ok(StreamWrapper::WriteOnly(Box::new(writer)))
                    },
                    AccessMode::ReadWrite => {
                        Ok(StreamWrapper::ReadWrite(Box::new(stream)))
                    }
                }
            },
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "Unsupported scheme")),
        }
    } else {
        // Assume it's a path to a FIFO or regular file
        let path = Path::new(source);
        
        #[cfg(windows)]
        {
            // Windows named pipe handling with retry logic
            
            const FILE_FLAG_OVERLAPPED: u32 = 0x40000000;
            const ERROR_PIPE_BUSY: i32 = 231;
            const PIPE_TIMEOUT_MS: u64 = 5000; // 5 seconds timeout
            
            // Try to open the pipe with multiple attempts
            let mut attempts = 0;
            let max_attempts = 10;
            
            loop {
                let mut options = OpenOptions::new();
                
                // Set access mode
                match mode {
                    AccessMode::Read => {
                        options.read(true);
                    },
                    AccessMode::Write => {
                        options.write(true);
                    },
                    AccessMode::ReadWrite => {
                        options.read(true).write(true);
                    }
                }
                
                // Add Windows-specific options
                options.custom_flags(FILE_FLAG_OVERLAPPED);
                
                let result = options.open(path);
                
                match result {
                    Ok(file) => {
                        match mode {
                            AccessMode::Read => return Ok(StreamWrapper::ReadOnly(Box::new(file))),
                            AccessMode::Write => return Ok(StreamWrapper::WriteOnly(Box::new(file))),
                            AccessMode::ReadWrite => return Ok(StreamWrapper::ReadWrite(Box::new(file))),
                        }
                    },
                    Err(e) => {
                        // Check if this is the "pipe busy" error
                        if e.raw_os_error() == Some(ERROR_PIPE_BUSY) {
                            attempts += 1;
                            
                            if attempts >= max_attempts {
                                warn!("Failed to open pipe after {} attempts: all pipe instances are busy", max_attempts);
                                return Err(e);
                            }
                            
                            // Wait before retrying
                            let wait_time = Duration::from_millis(PIPE_TIMEOUT_MS / max_attempts);
                            debug!("Pipe busy, waiting {}ms before retry attempt {}/{}", 
                                  wait_time.as_millis(), attempts, max_attempts);
                            thread::sleep(wait_time);
                            continue;
                        } else {
                            // For other errors, return immediately
                            return Err(e);
                        }
                    }
                }
            }
        }
          #[cfg(not(windows))]
        {
            match mode {
                // Opening a FIFO for read is meant to block until a writer
                // shows up -- callers that use it (the metadata pipe reader)
                // run it on their own dedicated thread and want to wait.
                AccessMode::Read => {
                    let file = OpenOptions::new().read(true).open(path)?;
                    Ok(StreamWrapper::ReadOnly(Box::new(file)))
                },
                // Opening a FIFO for write blocks the same way, but the only
                // caller (RAAT's control pipe) does this from a request
                // handler: a backend with nothing reading the other end must
                // not be able to hang that request forever.
                AccessMode::Write => {
                    let file = open_fifo_for_write(path)?;
                    Ok(StreamWrapper::WriteOnly(Box::new(file)))
                },
                AccessMode::ReadWrite => {
                    let file = OpenOptions::new().read(true).write(true).open(path)?;
                    Ok(StreamWrapper::ReadWrite(Box::new(file)))
                }
            }
        }
    }
}

/// How long to wait for a reader to show up on the other end of a FIFO
/// before giving up on opening it for write.
#[cfg(not(windows))]
const FIFO_WRITE_OPEN_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(not(windows))]
const FIFO_WRITE_OPEN_RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// Open `path` for writing without blocking forever when nothing has it open
/// for reading.
///
/// A plain blocking `open()` on a FIFO's write end waits until a reader
/// attaches, which is indefinite if the process that is supposed to read it
/// (e.g. a disabled/dead backend) never does. `O_NONBLOCK` changes that:
/// opening for write-only with no reader present fails immediately with
/// `ENXIO` instead of blocking, so this polls with `O_NONBLOCK` until a
/// reader appears or `FIFO_WRITE_OPEN_TIMEOUT` elapses, then hands back an
/// ordinary blocking file so the write itself behaves normally.
#[cfg(not(windows))]
fn open_fifo_for_write(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;

    let deadline = std::time::Instant::now() + FIFO_WRITE_OPEN_TIMEOUT;

    loop {
        match OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
        {
            Ok(file) => {
                // A reader is attached now; drop O_NONBLOCK so the write that
                // follows behaves like an ordinary blocking write.
                let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
                if flags >= 0 {
                    unsafe {
                        libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags & !libc::O_NONBLOCK);
                    }
                }
                return Ok(file);
            }
            Err(e) if e.raw_os_error() == Some(libc::ENXIO) => {
                if std::time::Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!(
                            "timed out after {:?} waiting for a reader on {}",
                            FIFO_WRITE_OPEN_TIMEOUT,
                            path.display()
                        ),
                    ));
                }
                thread::sleep(FIFO_WRITE_OPEN_RETRY_INTERVAL);
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn make_fifo(path: &Path) {
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let ret = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
        assert_eq!(ret, 0, "mkfifo failed: {}", io::Error::last_os_error());
    }

    /// This is the bug in issue #52: a backend (RAAT) registered as a player
    /// with nothing reading its control FIFO must not hang the caller.
    #[test]
    fn opening_a_fifo_for_write_with_no_reader_times_out_instead_of_hanging() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fifo_path = dir.path().join("control");
        make_fifo(&fifo_path);

        let started = std::time::Instant::now();
        let result = open_stream(fifo_path.to_str().unwrap(), AccessMode::Write);
        let elapsed = started.elapsed();

        match result {
            Err(e) => assert_eq!(e.kind(), io::ErrorKind::TimedOut),
            Ok(_) => panic!("expected a timeout error, opened a FIFO with no reader"),
        }
        assert!(
            elapsed < Duration::from_secs(5),
            "took {:?}, should have given up around {:?}",
            elapsed,
            FIFO_WRITE_OPEN_TIMEOUT
        );
    }

    #[test]
    fn opening_a_fifo_for_write_succeeds_once_a_reader_attaches() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fifo_path = dir.path().join("control");
        make_fifo(&fifo_path);

        let reader_path = fifo_path.clone();
        let reader = thread::spawn(move || {
            let mut file = OpenOptions::new().read(true).open(&reader_path).unwrap();
            let mut received = String::new();
            file.read_to_string(&mut received).ok();
            received
        });

        let mut stream = open_stream(fifo_path.to_str().unwrap(), AccessMode::Write)
            .expect("should open once a reader is attached");
        write!(stream.as_writer().unwrap(), "pause").unwrap();
        drop(stream);

        assert_eq!(reader.join().unwrap(), "pause");
    }
}