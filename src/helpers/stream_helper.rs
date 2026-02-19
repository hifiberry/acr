use std::io::{self, Read, Write};
use std::fs::OpenOptions;
use std::path::Path;
use std::net::TcpStream;
use url::Url;
use log::debug;

#[cfg(windows)]
use std::thread;
#[cfg(windows)]
use std::time::Duration;
#[cfg(windows)]
use log::warn;

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
            // Unix file/FIFO handling
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
            
            // For FIFOs/named pipes, we need to ensure proper blocking behavior
            // Check if this looks like a FIFO path (contains "pipe" in the name)
            // and we're opening for read access
            let is_fifo_read = matches!(mode, AccessMode::Read | AccessMode::ReadWrite) 
                && (source.contains("pipe") || source.contains("fifo"));
            
            if is_fifo_read {
                debug!("Opening what appears to be a FIFO for reading: {}", source);
                // For FIFOs, we want to ensure blocking behavior
                // Don't set O_NONBLOCK - this ensures the open() call itself may block
                // until a writer is available, which is the desired behavior for pipes
            }
            
            let file = options.open(path)?;
            
            match mode {
                AccessMode::Read => Ok(StreamWrapper::ReadOnly(Box::new(file))),
                AccessMode::Write => Ok(StreamWrapper::WriteOnly(Box::new(file))),
                AccessMode::ReadWrite => Ok(StreamWrapper::ReadWrite(Box::new(file))),
            }
        }
    }
}