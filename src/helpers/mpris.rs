#![cfg(unix)]

use dbus::blocking::{Connection, Proxy};
use dbus::arg::RefArg;
use std::collections::HashMap;
use std::time::Duration;
use log::info;

/// MPRIS player information
#[derive(Debug, Clone)]
pub struct MprisPlayer {
    pub bus_name: String,
    pub bus_type: BusType,
    pub identity: Option<String>,
    pub desktop_entry: Option<String>,
    pub can_control: Option<bool>,
    pub can_play: Option<bool>,
    pub can_pause: Option<bool>,
    pub can_seek: Option<bool>,
    pub can_go_next: Option<bool>,
    pub can_go_previous: Option<bool>,
    pub playback_status: Option<String>,
    pub current_track: Option<String>,
    pub current_artist: Option<String>,
}

/// Bus type enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum BusType {
    Session,
    System,
}

impl std::fmt::Display for BusType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BusType::Session => write!(f, "session"),
            BusType::System => write!(f, "system"),
        }
    }
}

/// Find MPRIS players on the specified bus
pub fn find_mpris_players(bus_type: BusType) -> Result<Vec<MprisPlayer>, Box<dyn std::error::Error>> {
    info!("Scanning for MPRIS players on {} bus", bus_type);
    
    let conn = match bus_type {
        BusType::Session => Connection::new_session()?,
        BusType::System => Connection::new_system()?,
    };
    
    // Get list of all services on the bus
    let proxy = Proxy::new("org.freedesktop.DBus", "/org/freedesktop/DBus", Duration::from_millis(5000), &conn);
    let (services,): (Vec<String>,) = proxy.method_call("org.freedesktop.DBus", "ListNames", ())?;
    
    let mut players = Vec::new();
    
    // Filter for MPRIS players
    for service in services {
        if service.starts_with("org.mpris.MediaPlayer2.") && service != "org.mpris.MediaPlayer2" {
            info!("Found potential MPRIS player: {}", service);
            
            match get_player_info(&conn, &service, bus_type.clone()) {
                Ok(player) => players.push(player),
                Err(e) => {
                    info!("Failed to get info for player {}: {}", service, e);
                    // Still add a basic entry even if we can't get full info
                    players.push(MprisPlayer {
                        bus_name: service,
                        bus_type: bus_type.clone(),
                        identity: None,
                        desktop_entry: None,
                        can_control: None,
                        can_play: None,
                        can_pause: None,
                        can_seek: None,
                        can_go_next: None,
                        can_go_previous: None,
                        playback_status: None,
                        current_track: None,
                        current_artist: None,
                    });
                }
            }
        }
    }
    
    info!("Found {} MPRIS players on {} bus", players.len(), bus_type);
    Ok(players)
}

/// Get detailed information about an MPRIS player
pub fn get_player_info(conn: &Connection, bus_name: &str, bus_type: BusType) -> Result<MprisPlayer, Box<dyn std::error::Error>> {
    let proxy = Proxy::new(bus_name, "/org/mpris/MediaPlayer2", Duration::from_millis(2000), conn);
    
    let mut player = MprisPlayer {
        bus_name: bus_name.to_string(),
        bus_type,
        identity: None,
        desktop_entry: None,
        can_control: None,
        can_play: None,
        can_pause: None,
        can_seek: None,
        can_go_next: None,
        can_go_previous: None,
        playback_status: None,
        current_track: None,
        current_artist: None,
    };
    
    // Helper function to get a property safely
    let get_property = |interface: &str, property: &str| -> Option<dbus::arg::Variant<Box<dyn RefArg>>> {
        proxy.method_call("org.freedesktop.DBus.Properties", "Get", (interface, property))
            .map(|(variant,): (dbus::arg::Variant<Box<dyn RefArg>>,)| variant)
            .ok()
    };
    
    // Get MediaPlayer2 properties
    if let Some(identity_variant) = get_property("org.mpris.MediaPlayer2", "Identity") {
        if let Some(identity) = identity_variant.as_str() {
            player.identity = Some(identity.to_string());
        }
    }
    
    if let Some(desktop_entry_variant) = get_property("org.mpris.MediaPlayer2", "DesktopEntry") {
        if let Some(desktop_entry) = desktop_entry_variant.as_str() {
            player.desktop_entry = Some(desktop_entry.to_string());
        }
    }
    
    // Get Player properties  
    if let Some(can_control_variant) = get_property("org.mpris.MediaPlayer2.Player", "CanControl") {
        if let Some(can_control) = can_control_variant.as_u64().map(|v| v != 0)
            .or_else(|| can_control_variant.as_i64().map(|v| v != 0)) {
            player.can_control = Some(can_control);
        }
    }
    
    if let Some(can_play_variant) = get_property("org.mpris.MediaPlayer2.Player", "CanPlay") {
        if let Some(can_play) = can_play_variant.as_u64().map(|v| v != 0)
            .or_else(|| can_play_variant.as_i64().map(|v| v != 0)) {
            player.can_play = Some(can_play);
        }
    }
    
    if let Some(can_pause_variant) = get_property("org.mpris.MediaPlayer2.Player", "CanPause") {
        if let Some(can_pause) = can_pause_variant.as_u64().map(|v| v != 0)
            .or_else(|| can_pause_variant.as_i64().map(|v| v != 0)) {
            player.can_pause = Some(can_pause);
        }
    }
    
    if let Some(can_seek_variant) = get_property("org.mpris.MediaPlayer2.Player", "CanSeek") {
        if let Some(can_seek) = can_seek_variant.as_u64().map(|v| v != 0)
            .or_else(|| can_seek_variant.as_i64().map(|v| v != 0)) {
            player.can_seek = Some(can_seek);
        }
    }
    
    if let Some(can_go_next_variant) = get_property("org.mpris.MediaPlayer2.Player", "CanGoNext") {
        if let Some(can_go_next) = can_go_next_variant.as_u64().map(|v| v != 0)
            .or_else(|| can_go_next_variant.as_i64().map(|v| v != 0)) {
            player.can_go_next = Some(can_go_next);
        }
    }
    
    if let Some(can_go_previous_variant) = get_property("org.mpris.MediaPlayer2.Player", "CanGoPrevious") {
        if let Some(can_go_previous) = can_go_previous_variant.as_u64().map(|v| v != 0)
            .or_else(|| can_go_previous_variant.as_i64().map(|v| v != 0)) {
            player.can_go_previous = Some(can_go_previous);
        }
    }
    
    if let Some(playback_status_variant) = get_property("org.mpris.MediaPlayer2.Player", "PlaybackStatus") {
        if let Some(playback_status) = playback_status_variant.as_str() {
            player.playback_status = Some(playback_status.to_string());
        }
    }
    
    // Get metadata
    if let Some(metadata_variant) = get_property("org.mpris.MediaPlayer2.Player", "Metadata") {
        if let Some(metadata_iter) = metadata_variant.as_iter() {
            let mut metadata_map = HashMap::new();
            
            // Parse the metadata dictionary
            let mut iter = metadata_iter;
            while let (Some(key), Some(value)) = (iter.next(), iter.next()) {
                if let Some(key_str) = key.as_str() {
                    metadata_map.insert(key_str.to_string(), value);
                }
            }
            
            // Extract title
            if let Some(title_variant) = metadata_map.get("xesam:title") {
                if let Some(title) = title_variant.as_str() {
                    player.current_track = Some(title.to_string());
                }
            }
            
            // Extract artist (usually an array)
            if let Some(artist_variant) = metadata_map.get("xesam:artist") {
                if let Some(mut artists) = artist_variant.as_iter() {
                    if let Some(first_artist) = artists.next() {
                        if let Some(artist) = first_artist.as_str() {
                            player.current_artist = Some(artist.to_string());
                        }
                    }
                } else if let Some(artist) = artist_variant.as_str() {
                    // Some implementations might return a single string instead of array
                    player.current_artist = Some(artist.to_string());
                }
            }
        }
    }
    
    Ok(player)
}

/// Create a connection to the specified bus type
pub fn create_connection(bus_type: BusType) -> Result<Connection, Box<dyn std::error::Error>> {
    match bus_type {
        BusType::Session => Ok(Connection::new_session()?),
        BusType::System => Ok(Connection::new_system()?),
    }
}

/// Create a proxy for an MPRIS player
pub fn create_player_proxy<'a>(conn: &'a Connection, bus_name: &'a str) -> Proxy<'a, &'a Connection> {
    Proxy::new(bus_name, "/org/mpris/MediaPlayer2", Duration::from_millis(2000), conn)
}

/// Helper function to get a D-Bus property safely
pub fn get_dbus_property(proxy: &Proxy<'_, &Connection>, interface: &str, property: &str) -> Option<dbus::arg::Variant<Box<dyn RefArg>>> {
    proxy.method_call("org.freedesktop.DBus.Properties", "Get", (interface, property))
        .map(|(variant,): (dbus::arg::Variant<Box<dyn RefArg>>,)| variant)
        .ok()
}

/// Send a method call to an MPRIS player
pub fn send_player_method(proxy: &Proxy<'_, &Connection>, method: &str) -> Result<(), Box<dyn std::error::Error>> {
    proxy.method_call::<(), (), _, _>("org.mpris.MediaPlayer2.Player", method, ())?;
    Ok(())
}

/// Send a method call with arguments to an MPRIS player
pub fn send_player_method_with_args<A>(proxy: &Proxy<'_, &Connection>, method: &str, args: A) -> Result<(), Box<dyn std::error::Error>>
where
    A: dbus::arg::AppendAll,
{
    proxy.method_call::<(), A, _, _>("org.mpris.MediaPlayer2.Player", method, args)?;
    Ok(())
}

/// Set a D-Bus property on an MPRIS player
pub fn set_player_property<V>(proxy: &Proxy<'_, &Connection>, property: &str, value: V) -> Result<(), Box<dyn std::error::Error>>
where
    V: dbus::arg::Append + dbus::arg::Arg + Clone,
{
    proxy.method_call::<(), _, _, _>("org.freedesktop.DBus.Properties", "Set", 
        ("org.mpris.MediaPlayer2.Player", property, dbus::arg::Variant(value)))?;
    Ok(())
}

/// Extract metadata from a D-Bus metadata dictionary
pub fn extract_metadata(metadata_variant: &dbus::arg::Variant<Box<dyn RefArg>>) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    
    if let Some(metadata_iter) = metadata_variant.as_iter() {
        let mut iter = metadata_iter;
        while let (Some(key), Some(value)) = (iter.next(), iter.next()) {
            if let Some(key_str) = key.as_str() {
                let value_str = if let Some(val) = value.as_str() {
                    val.to_string()
                } else if let Some(mut artists) = value.as_iter() {
                    // Handle array of artists
                    if let Some(first_artist) = artists.next() {
                        first_artist.as_str().unwrap_or("").to_string()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                metadata.insert(key_str.to_string(), value_str);
            }
        }
    }
    
    metadata
}

/// Helper function to convert a boolean value to D-Bus format
pub fn bool_to_dbus_variant(value: bool) -> dbus::arg::Variant<bool> {
    dbus::arg::Variant(value)
}

/// Helper function to convert a string value to D-Bus format
pub fn string_to_dbus_variant(value: &str) -> dbus::arg::Variant<String> {
    dbus::arg::Variant(value.to_string())
}

/// Helper function to convert an i64 value to D-Bus format  
pub fn i64_to_dbus_variant(value: i64) -> dbus::arg::Variant<i64> {
    dbus::arg::Variant(value)
}

/// Helper function to convert a f64 value to D-Bus format
pub fn f64_to_dbus_variant(value: f64) -> dbus::arg::Variant<f64> {
    dbus::arg::Variant(value)
}

/// Get a specific property from an MPRIS player as a string
pub fn get_string_property(proxy: &Proxy<'_, &Connection>, interface: &str, property: &str) -> Option<String> {
    get_dbus_property(proxy, interface, property)?
        .as_str()
        .map(|s| s.to_string())
}

/// Get a specific property from an MPRIS player as a boolean
pub fn get_bool_property(proxy: &Proxy<'_, &Connection>, interface: &str, property: &str) -> Option<bool> {
    let variant = get_dbus_property(proxy, interface, property)?;
    
    // Try as u64 first, then i64
    variant.as_u64().map(|v| v != 0)
        .or_else(|| variant.as_i64().map(|v| v != 0))
}

/// Get a specific property from an MPRIS player as an i64
pub fn get_i64_property(proxy: &Proxy<'_, &Connection>, interface: &str, property: &str) -> Option<i64> {
    get_dbus_property(proxy, interface, property)?
        .as_i64()
}

/// Get a specific property from an MPRIS player as an f64
pub fn get_f64_property(proxy: &Proxy<'_, &Connection>, interface: &str, property: &str) -> Option<f64> {
    get_dbus_property(proxy, interface, property)?
        .as_f64()
}

/// Check if a player exists on the bus
pub fn player_exists(conn: &Connection, bus_name: &str) -> bool {
    let proxy = Proxy::new("org.freedesktop.DBus", "/org/freedesktop/DBus", Duration::from_millis(1000), conn);
    
    proxy.method_call::<(bool,), _, _, _>("org.freedesktop.DBus", "NameHasOwner", (bus_name,))
        .map(|(exists,)| exists)
        .unwrap_or(false)
}

/// Find a specific player by name or return the first available player
pub fn find_player_by_name_or_first(bus_type: BusType, player_name: Option<&str>) -> Result<Option<MprisPlayer>, Box<dyn std::error::Error>> {
    let players = find_mpris_players(bus_type)?;
    
    if let Some(name) = player_name {
        // Look for specific player
        for player in players {
            if player.bus_name.contains(name) || 
               player.identity.as_ref().map_or(false, |id| id.contains(name)) {
                return Ok(Some(player));
            }
        }
        Ok(None)
    } else {
        // Return first available player
        Ok(players.into_iter().next())
    }
}
