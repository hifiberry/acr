#![cfg(unix)]

use clap::Parser;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use audiocontrol::helpers::shairportsync_messages::{
    ShairportMessage, ChunkCollector, parse_shairport_message, 
    detect_image_format, get_image_dimensions, get_jpeg_dimensions, get_png_dimensions
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

fn print_hex_dump(data: &[u8], prefix: &str) {
    for (i, chunk) in data.chunks(16).enumerate() {
        print!("{}{:04x}: ", prefix, i * 16);
        
        // Print hex values
        for (j, byte) in chunk.iter().enumerate() {
            print!("{:02x} ", byte);
            if j == 7 {
                print!(" "); // Extra space in the middle
            }
        }
        
        // Pad if this chunk is less than 16 bytes
        for j in chunk.len()..16 {
            print!("   ");
            if j == 7 {
                print!(" ");
            }
        }
        
        print!(" |");
        
        // Print ASCII representation
        for byte in chunk {
            if byte.is_ascii_graphic() || *byte == b' ' {
                print!("{}", *byte as char);
            } else {
                print!(".");
            }
        }
        
        println!("|");
    }
}

fn display_shairport_message(message: &ShairportMessage, show_hex: bool) {
    match message {
        ShairportMessage::Control(action) => {
            println!("  {}", action);
        }
        
        ShairportMessage::SessionStart(session_id) => {
            println!("  SESSION START: {}", session_id);
        }
        
        ShairportMessage::SessionEnd(timestamp) => {
            println!("  SESSION END: {}", timestamp);
        }
        
        ShairportMessage::CompletePicture { data, format } => {
            println!("  COMPLETE PICTURE:");
            println!("     Format: {}", format);
            println!("     Size: {} bytes", data.len());
            println!("     Dimensions: {}", get_image_dimensions(data, format));
            
            if show_hex && data.len() <= 256 {
                println!("     Hex dump (header):");
                print_hex_dump(data, "       ");
            } else if show_hex {
                println!("     Hex dump (first 256 bytes):");
                print_hex_dump(&data[..256], "       ");
            }
        }
        
        ShairportMessage::ChunkData { chunk_id, total_chunks, data_type, data } => {
            println!("  CHUNK DATA:");
            println!("     Type: {}", data_type.trim_end_matches('\0'));
            println!("     Chunk: {}/{}", chunk_id, total_chunks);
            
            if data.is_empty() {
                println!("     Size: 0 bytes (header/padding only)");
            } else {
                println!("     Size: {} bytes", data.len());
            }
            
            // Special handling for different data types
            let clean_type = data_type.trim_end_matches('\0');
            match clean_type {
                "ssncPICT" => {
                    if data.is_empty() {
                        println!("     Content: Album artwork header (no data in this chunk)");
                    } else {
                        let format = detect_image_format(data);
                        println!("     Content: Album artwork ({})", format);
                        println!("     Format: {} detected", format);
                        
                        if format.contains("JPEG") {
                            let dimensions = get_jpeg_dimensions(data);
                            if dimensions != "Unknown" {
                                println!("     Dimensions: {}", dimensions);
                            }
                        } else if format.contains("PNG") {
                            let dimensions = get_png_dimensions(data);
                            if dimensions != "Unknown" {
                                println!("     Dimensions: {}", dimensions);
                            }
                        }
                    }
                }
                "ssncminu" => println!("     Content: Metadata - Track info"),
                "ssncasar" => println!("     Content: Metadata - Artist"),
                "ssncasal" => println!("     Content: Metadata - Album"),
                "ssncastn" => println!("     Content: Metadata - Track name"),
                _ => {
                    if clean_type.starts_with("ssnc") {
                        println!("     Content: Metadata - {}", &clean_type[4..]);
                    } else {
                        println!("     Content: Unknown data type");
                    }
                }
            }
            
            if !data.is_empty() && show_hex {
                if data.len() <= 256 {
                    println!("     Hex dump:");
                    print_hex_dump(data, "       ");
                } else {
                    println!("     Hex dump (first 256 bytes):");
                    print_hex_dump(&data[..256], "       ");
                }
            }
        }
        
        ShairportMessage::Unknown(data) => {
            // Try to display as text if it looks like text, but always show hex dump
            if let Ok(text) = std::str::from_utf8(data) {
                if text.chars().all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace()) {
                    println!("  UNKNOWN TEXT: {}", text.trim());
                    println!("  Hex dump:");
                    print_hex_dump(data, "     ");
                    return;
                }
            }
            
            println!("  UNKNOWN BINARY DATA: {} bytes", data.len());
            print_hex_dump(data, "     ");
        }
    }
}
