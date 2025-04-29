use named_pipe::{PipeOptions, PipeClient};
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};
use std::env;
use std::process;
use std::path::Path;

fn print_usage() {
    eprintln!("USAGE: pipebridge full_pipe_path:host:port");
    eprintln!("Example: pipebridge \\\\.\\pipe\\my_pipe:192.168.1.100:12345");
    eprintln!("");
    eprintln!("Parameters:");
    eprintln!("  full_pipe_path  - Windows named pipe path (e.g. \\\\.\\pipe\\my_pipe)");
    eprintln!("  host            - Remote host IP address or hostname");
    eprintln!("  port            - Remote host port number");
    eprintln!("");
    eprintln!("Note: Pipe names must be in the format \\\\.\\pipe\\pipename");
    eprintln!("      Relative paths like ./pipe1 will be automatically converted to \\\\.\\pipe\\pipe1");
}

struct ConnectionInfo {
    pipe_name: String,
    host: String,
    port: u16,
}

// Function to normalize pipe path to Windows format
fn normalize_pipe_path(path: &str) -> String {
    // Check if the path is already in the Windows pipe format
    if path.starts_with(r"\\.\pipe\") || path.starts_with(r"//./pipe/") {
        return path.to_string();
    }
    
    // Extract the pipe name from the path by getting the file name component
    let pipe_name = Path::new(path).file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| {
            // If we can't extract a filename, use the original path as the pipe name
            path.replace("/", "_").replace("\\", "_")
        });
    
    // Create a proper Windows named pipe path
    format!(r"\\.\pipe\{}", pipe_name)
}

fn parse_connection_string(conn_str: &str) -> Option<ConnectionInfo> {
    let parts: Vec<&str> = conn_str.split(':').collect();
    
    // We expect at least 3 parts: pipe_path:host:port
    // The pipe path might contain colons (e.g. \\.\pipe\name), so we need to handle that
    if parts.len() < 3 {
        eprintln!("Invalid format. Expected full_pipe_path:host:port");
        return None;
    }
    
    // The last two parts are host and port
    let port_str = parts.last().unwrap();
    let host = parts[parts.len() - 2];
    
    // Everything before the last two parts is the pipe path
    let pipe_path_parts = &parts[0..parts.len() - 2];
    let raw_pipe_name = pipe_path_parts.join(":");
    
    // Normalize the pipe name to ensure it works on Windows
    let pipe_name = normalize_pipe_path(&raw_pipe_name);
    
    // Parse the port
    let port = match port_str.parse::<u16>() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Invalid port number: {}", port_str);
            return None;
        }
    };
    
    Some(ConnectionInfo {
        pipe_name,
        host: host.to_string(),
        port,
    })
}

#[tokio::main]
async fn main() -> io::Result<()> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    
    // Check if arguments were provided, abort if not
    if args.len() <= 1 {
        eprintln!("Error: No connection information provided");
        print_usage();
        process::exit(1);
    }
    
    let connection_info = match parse_connection_string(&args[1]) {
        Some(info) => info,
        None => {
            print_usage();
            process::exit(1);
        }
    };
    
    let pipe_name = &connection_info.pipe_name;
    let tcp_addr = format!("{}:{}", connection_info.host, connection_info.port);
    
    println!("Creating named pipe: {}", pipe_name);
    println!("Will connect to TCP server at: {}", tcp_addr);

    // Create named pipe server with better error handling
    let server_pipe_name = pipe_name;
    let pipe_server = match PipeOptions::new(server_pipe_name).single() {
        Ok(server) => server,
        Err(e) => {
            eprintln!("Failed to create named pipe: {}", e);
            eprintln!("Make sure the pipe name is in the format \\\\.\\pipe\\pipename");
            process::exit(1);
        }
    };
    
    println!("Waiting for client to connect to pipe...");
    
    // Wait for a client connection with better error handling
    let server_connection = match pipe_server.wait() {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("Error waiting for client connection: {}", e);
            process::exit(1);
        }
    };
    
    println!("Client connected to pipe server.");
    
    // Create a client connection back to our own pipe for writing
    let client_pipe_name = pipe_name.to_owned();
    let client_connection = match named_pipe::PipeClient::connect(client_pipe_name) {
        Ok(client) => {
            println!("Created pipe client connection.");
            client
        },
        Err(e) => {
            eprintln!("Failed to connect client to pipe: {}", e);
            return Err(e);
        }
    };

    let tcp = TcpStream::connect(&tcp_addr).await?;
    println!("Connected to TCP server at {}", tcp_addr);

    // Use the server connection for reading and client connection for writing
    let pipe_reader = Arc::new(Mutex::<named_pipe::PipeServer>::new(server_connection));
    let pipe_writer = Arc::new(Mutex::<PipeClient>::new(client_connection));
    
    // Split TCP stream for bidirectional copy
    let (mut tcp_reader, tcp_writer) = tokio::io::split(tcp);
    
    // Create thread for pipe -> TCP
    let pipe_r = pipe_reader.clone();
    let tcp_w = Arc::new(Mutex::new(tcp_writer));
    
    let pipe_to_tcp = tokio::task::spawn_blocking(move || {
        let mut buffer = [0u8; 4096];
        loop {
            // Read from pipe
            let read_result = {
                let mut reader = pipe_r.lock().unwrap();
                reader.read(&mut buffer)
            };
            
            match read_result {
                Ok(n) if n > 0 => {
                    // Write to TCP
                    let mut writer = tcp_w.lock().unwrap();
                    if let Err(e) = futures::executor::block_on(async {
                        writer.write_all(&buffer[0..n]).await
                    }) {
                        eprintln!("Error writing to TCP: {}", e);
                        break;
                    }
                },
                Ok(0) => {
                    println!("Pipe closed");
                    break;
                },
                Ok(_) => {
                    // This case handles any other positive read size
                    // In practice, this shouldn't happen as we've already handled n > 0
                    continue;
                },
                Err(e) => {
                    eprintln!("Error reading from pipe: {}", e);
                    break;
                }
            }
        }
    });

    // Create thread for TCP -> pipe
    let tcp_to_pipe = tokio::spawn(async move {
        let mut buffer = [0u8; 4096];
        loop {
            // Read from TCP
            match tcp_reader.read(&mut buffer).await {
                Ok(n) if n > 0 => {
                    // Write to pipe
                    let mut writer = pipe_writer.lock().unwrap();
                    if let Err(e) = writer.write_all(&buffer[0..n]) {
                        eprintln!("Error writing to pipe: {}", e);
                        break;
                    }
                },
                Ok(0) => {
                    println!("TCP connection closed");
                    break;
                },
                Ok(_) => {
                    // This case handles any other positive read size
                    // In practice, this shouldn't happen as we've already handled n > 0
                    continue;
                },
                Err(e) => {
                    eprintln!("Error reading from TCP: {}", e);
                    break;
                }
            }
        }
    });

    // Wait for both threads
    let _ = tokio::join!(pipe_to_tcp, tcp_to_pipe);
    println!("Bridge closed");
    
    Ok(())
}