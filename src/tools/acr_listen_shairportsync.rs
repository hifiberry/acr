#![cfg(unix)]

use clap::Parser;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use audiocontrol::helpers::shairportsync_messages::{
    ShairportMessage, ChunkCollector, parse_shairport_message, 
    display_shairport_message, detect_image_format
};

#[derive(Parser)]
#[command(name = "audiocontrol_listen_shairportsync")]
#[command(about = "AudioControl ShairportSync UDP Listener")]
#[command(long_about = "Listens for UDP packets on the specified port and displays their content.\nPackets are assumed to be text and will be displayed as such. If binary\ndata is received, it will be shown as a hex dump.\n\nThis tool is useful for monitoring ShairportSync metadata or other\nUDP-based communication. Press Ctrl+C to stop listening.")]
#[command(version)]
struct Args {
    /// UDP port to listen on
    #[arg(long, default_value_t = 5555)]
    port: u16,
    
    /// Show raw hex dump for binary data
    #[arg(long, default_value_t = false)]
    show_hex: bool,
}

fn main() {
    env_logger::init();
    
    let args = Args::parse();
    let port = args.port;
    let show_hex = args.show_hex;
    
    println!("AudioControl ShairportSync UDP Listener");
    println!("=====================================");
    println!("Listening on UDP port: {}", port);
    println!("Press Ctrl+C to stop...");
    println!();
    
    // Set up signal handler for Ctrl+C
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    
    ctrlc::set_handler(move || {
        println!("\nReceived Ctrl+C, shutting down...");
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");
    
    // Bind to UDP socket
    let bind_address = format!("0.0.0.0:{}", port);
    let socket = match UdpSocket::bind(&bind_address) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: Failed to bind to {}: {}", bind_address, e);
            std::process::exit(1);
        }
    };
    
    println!("Successfully bound to {}", bind_address);
    println!("Waiting for packets...");
    println!();
    
    // Set socket timeout to allow checking the running flag
    socket.set_read_timeout(Some(std::time::Duration::from_millis(1000)))
        .expect("Failed to set socket timeout");
    
    let mut buffer = [0; 4096]; // 4KB buffer for incoming packets
    let mut packet_count = 0;
    let mut picture_collector: Option<ChunkCollector> = None;
    
    while running.load(Ordering::SeqCst) {
        match socket.recv_from(&mut buffer) {
            Ok((bytes_received, sender_addr)) => {
                packet_count += 1;
                
                // Get current timestamp
                let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                
                println!("[{}] Packet #{} from {} ({} bytes):", 
                         timestamp, packet_count, sender_addr, bytes_received);
                
                // Parse ShairportSync message
                let mut message = parse_shairport_message(&buffer[..bytes_received]);
                
                // Handle chunk collection for pictures
                if let ShairportMessage::ChunkData { chunk_id, total_chunks, data_type, data } = &message {
                    let clean_type = data_type.trim_end_matches('\0');
                    
                    if clean_type == "ssncPICT" {
                        // Initialize collector if this is the first chunk
                        if picture_collector.is_none() {
                            picture_collector = Some(ChunkCollector::new(*total_chunks, clean_type.to_string()));
                        }
                        
                        // Add chunk to collector
                        if let Some(ref mut collector) = picture_collector {
                            if let Some(complete_data) = collector.add_chunk(*chunk_id, data.clone()) {
                                // We have a complete picture
                                let format = detect_image_format(&complete_data);
                                message = ShairportMessage::CompletePicture {
                                    data: complete_data,
                                    format,
                                };
                                picture_collector = None; // Reset for next picture
                            }
                        }
                    }
                }
                
                display_shairport_message(&message, show_hex);
                
                println!(); // Empty line between packets
            }
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                        // Timeout occurred, continue loop to check running flag
                        continue;
                    }
                    _ => {
                        eprintln!("Error receiving packet: {}", e);
                        break;
                    }
                }
            }
        }
    }
    
    println!("Listener stopped. Total packets received: {}", packet_count);
}
