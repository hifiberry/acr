use log::{debug, info, error};

use crate::players::lms::jsonrps::LmsRpcClient;
use crate::helpers::macaddress::{normalize_mac_address, mac_equal_ignore_case, mac_to_lowercase_string};

/// Check if any of the specified MAC addresses is connected to the given LMS server
///
/// # Arguments
/// * `server` - Server address (hostname or IP)
/// * `mac_addresses` - Vector of MAC addresses to check
///
/// # Returns
/// `true` if any of the MAC addresses is connected to the server, `false` otherwise
pub fn is_player(server: &str, mac_addresses: Vec<String>) -> bool {
    if mac_addresses.is_empty() {
        debug!("No MAC addresses provided to check");
        return false;
    }

    // Create a client for the server with default port (9000)
    let mut client = LmsRpcClient::new(server, 9000);

    // Get all players connected to the server
    let players = match client.get_players() {
        Ok(players) => players,
        Err(e) => {
            error!("Failed to get players from LMS server {}: {}", server, e);
            return false;
        }
    };

    debug!("Found {} players on LMS server {}", players.len(), server);

    // Check if any of the provided MAC addresses matches a connected player
    for player in players {
        debug!("Server player: {} (MAC: {})", player.name, player.playerid);

        // Normalize the player's MAC address for comparison
        match normalize_mac_address(&player.playerid) {
            Ok(player_mac) => {
                // Convert to lowercase string for consistent comparison
                let player_mac_str = mac_to_lowercase_string(&player_mac);

                // Check against each provided MAC address
                for mac in &mac_addresses {
                    if mac_equal_ignore_case(&player_mac_str, mac) {
                        debug!("Found matching player: {} with MAC {} on server {}", 
                            player.name, player_mac_str, server);
                        return true;
                    }
                }
            },
            Err(e) => {
                debug!("Failed to normalize MAC address {}: {}", player.playerid, e);
            }
        }
    }

    debug!("None of the provided MAC addresses are connected to LMS server {}", server);
    false
}

/// Find the first server from a list that has any of the specified MAC addresses connected
///
/// # Arguments
/// * `servers` - Vector of server addresses (hostname or IP)
/// * `mac_addresses` - Vector of MAC addresses to check
///
/// # Returns
/// `Some(server)` with the first server that has any of the MAC addresses connected,
/// or `None` if no server has any of the MAC addresses connected
pub fn find_my_server(servers: Vec<String>, mac_addresses: Vec<String>) -> Option<String> {
    if servers.is_empty() {
        debug!("No servers provided to check");
        return None;
    }

    if mac_addresses.is_empty() {
        debug!("No MAC addresses provided to check");
        return None;
    }

    debug!("Checking {} servers for {} MAC addresses", servers.len(), mac_addresses.len());
    
    for server in servers {
        debug!("Checking server: {}", server);
        if is_player(&server, mac_addresses.clone()) {
            info!("Found matching server: {}", server);
            return Some(server);
        }
    }

    debug!("No matching server found for the provided MAC addresses");
    None
}