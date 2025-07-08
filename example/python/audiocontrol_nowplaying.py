#!/usr/bin/env python3
"""
AudioControl Now Playing Display

A terminal-based now playing display for HiFiBerry AudioControl.
Features:
- Updates every 5 seconds with current song information (customizable with --interval)
- Displays artist, title, album, and playback progress
- Uses the full terminal size or a custom size (--size=WIDTHxHEIGHT)
- Shows a progress slider and audio format information
- Colorful ASCII frame with connection status indicator
- Custom API endpoint support (--url)

Usage:
  python audiocontrol_nowplaying.py [--url=URL] [--size=WIDTHxHEIGHT] [--interval=SECONDS]
"""

import os
import sys
import time
import json
import urllib.request
from urllib.error import URLError
import argparse

# Default configuration
UPDATE_INTERVAL = 5  # seconds

# ANSI Color Codes
class Colors:
    RESET = "\033[0m"
    BOLD = "\033[1m"
    UNDERLINE = "\033[4m"
    
    BLACK = "\033[30m"
    RED = "\033[31m"
    GREEN = "\033[32m"
    YELLOW = "\033[33m"
    BLUE = "\033[34m"
    MAGENTA = "\033[35m"
    CYAN = "\033[36m"
    WHITE = "\033[37m"
    
    BG_BLACK = "\033[40m"
    BG_RED = "\033[41m"
    BG_GREEN = "\033[42m"
    BG_YELLOW = "\033[43m"
    BG_BLUE = "\033[44m"
    BG_MAGENTA = "\033[45m"
    BG_CYAN = "\033[46m"
    BG_WHITE = "\033[47m"


def parse_arguments():
    """Parse command line arguments."""
    parser = argparse.ArgumentParser(description='AudioControl Now Playing Display')
    parser.add_argument('--url', type=str, default='http://localhost:1080/api',
                        help='Base URL of the AudioControl API (default: http://localhost:1080/api)')
    parser.add_argument('--size', type=str, 
                        help='Custom display size in format widthxheight (e.g. 80x24). If not specified, uses terminal size')
    parser.add_argument('--interval', type=int, default=5,
                        help='Update interval in seconds (default: 5)')
    return parser.parse_args()


def fetch_now_playing(api_base_url):
    """Fetch the current now playing information from the AudioControl API."""
    try:
        url = f"{api_base_url}/now-playing"
        
        # Set up a request with a timeout and user agent
        request = urllib.request.Request(
            url,
            headers={'User-Agent': 'AudioControlNowPlaying/1.0'}
        )
        
        # Open with a timeout of 5 seconds
        with urllib.request.urlopen(request, timeout=5) as response:
            data = response.read().decode('utf-8')
            result = json.loads(data)
            
            # Also fetch player information to show which player is active
            try:
                player_url = f"{api_base_url}/players"
                player_request = urllib.request.Request(
                    player_url,
                    headers={'User-Agent': 'AudioControlNowPlaying/1.0'}
                )
                with urllib.request.urlopen(player_request, timeout=5) as player_response:
                    player_data = json.loads(player_response.read().decode('utf-8'))
                    result["player_info"] = player_data
            except Exception:
                # Ignore errors fetching player info
                pass
                
            return result
    except urllib.error.HTTPError as e:
        print(f"\nHTTP Error: {e.code} {e.reason} - URL: {url}")
        return {"error": f"HTTP Error: {e.code} {e.reason}"}
    except urllib.error.URLError as e:
        print(f"\nURL Error: {e.reason} - URL: {url}")
        if isinstance(e.reason, str) and "SSL" in e.reason:
            print("Try using http:// instead of https:// in your URL")
        return {"error": f"Connection Error: {e.reason}"}
    except json.JSONDecodeError as e:
        print(f"\nInvalid JSON response from {url}: {e}")
        return {"error": f"Invalid JSON response"}
    except TimeoutError:
        print(f"\nConnection timeout when connecting to {url}")
        return {"error": "Connection timed out"}
    except Exception as e:
        print(f"\nUnexpected error connecting to {url}: {e}")
        return {"error": f"Error: {e}"}


def format_now_playing(data):
    """Format the now playing data for display."""
    if "error" in data:
        return {
            "artist": "Error",
            "title": data["error"],
            "album": "",
            "state": "error",
            "position": None,
            "duration": None,
            "player_name": "Unknown",
            "playback_type": "",
            "sample_rate": "",
            "bit_depth": ""
        }

    song = data.get("song", {})
    state = data.get("state", "unknown")
    
    # Extract additional information
    player_name = "Unknown"
    playback_type = ""
    sample_rate = ""
    bit_depth = ""
    
    # Get player name and other details
    if "player_info" in data and isinstance(data["player_info"], list):
        for player in data["player_info"]:
            if player.get("isPlaying", False):
                player_name = player.get("name", "Unknown")
                break
    
    # Extract audio quality information if available
    if "streamDetails" in song:
        stream = song.get("streamDetails", {})
        if stream:
            sample_rate = stream.get("sampleRate", "")
            bit_depth = stream.get("bitDepth", "")
            if sample_rate and bit_depth:
                playback_type = f"{bit_depth}bit/{sample_rate}kHz"
            elif "uri" in song:
                # Try to guess format from URI
                uri = song.get("uri", "")
                if "spotify:" in uri:
                    playback_type = "Spotify"
                elif any(fmt in uri.lower() for fmt in [".mp3", ".aac", ".ogg"]):
                    playback_type = "Compressed"
                elif any(fmt in uri.lower() for fmt in [".flac", ".wav", ".aif", ".dsd"]):
                    playback_type = "Lossless"
    
    return {
        "artist": song.get("artist", "Unknown Artist"),
        "title": song.get("title", "Unknown Title"),
        "album": song.get("album", "Unknown Album"),
        "state": state,
        "position": data.get("position"),
        "duration": song.get("duration"),
        "player_name": player_name,
        "playback_type": playback_type,
        "sample_rate": sample_rate,
        "bit_depth": bit_depth
    }


def create_frame(width, height):
    """Create a frame with the given width and height."""
    top_bottom = "╔" + "═" * (width - 2) + "╗"
    middle = "║" + " " * (width - 2) + "║"
    bottom = "╚" + "═" * (width - 2) + "╝"
    
    frame = [top_bottom]
    for _ in range(height - 2):
        frame.append(middle)
    frame.append(bottom)
    
    return frame


def update_frame(frame, info, width, height):
    """Update the frame with the now playing information."""
    # Calculate center positions
    artist_pos = height // 3
    title_pos = height // 2
    album_pos = (height * 2) // 3
    progress_pos = min(height - 4, album_pos + 2)  # Position for progress bar
    state_pos = min(height - 3, album_pos + 3)  # Ensure it's within the frame
    
    # Center-align text within the frame
    def center_text(text, width):
        text = str(text)
        padding = width - len(text) - 2  # -2 for the frame borders
        left_pad = padding // 2
        right_pad = padding - left_pad
        return "║" + " " * left_pad + text + " " * right_pad + "║"

    # Add a header line with the date and time
    player_info = ""
    if info["player_name"] != "Unknown":
        player_info = f" - {info['player_name']}"
    if info["playback_type"]:
        player_info += f" - {info['playback_type']}"
        
    header_text = f"AudioControl Now Playing{player_info} - {time.strftime('%Y-%m-%d %H:%M:%S')}"
    frame[1] = center_text(header_text, width)
    
    # Artist
    artist_line = center_text(info["artist"], width)
    frame[artist_pos] = Colors.CYAN + artist_line + Colors.RESET
    
    # Title
    title_line = center_text(info["title"], width)
    frame[title_pos] = Colors.YELLOW + Colors.BOLD + title_line + Colors.RESET
    
    # Album
    album_line = center_text(info["album"], width)
    frame[album_pos] = Colors.GREEN + album_line + Colors.RESET
    
    # Progress bar
    if info["duration"] and info["position"] is not None:
        # Calculate progress percentage
        progress_percent = min(1.0, info["position"] / info["duration"])
        
        # Create progress bar with a reasonable width (80% of the terminal width)
        bar_width = int((width - 16) * 0.8)  # Leave some space for the frame and time
        filled_width = int(bar_width * progress_percent)
        empty_width = bar_width - filled_width
        
        # Format position and duration
        position_min = int(info["position"]) // 60
        position_sec = int(info["position"]) % 60
        duration_min = int(info["duration"]) // 60
        duration_sec = int(info["duration"]) % 60
        
        time_display = f"{position_min:02d}:{position_sec:02d}"
        duration_display = f"{duration_min:02d}:{duration_sec:02d}"
        
        # Enhanced progress bar with slider indicator
        if filled_width > 0:
            filled_chars = '━' * (filled_width - 1)
            slider_char = '⊙'  # Slider character
            progress_bar = f"{time_display} [{Colors.CYAN}{filled_chars}{slider_char}{Colors.RESET}{'─' * empty_width}] {duration_display}"
        else:
            slider_char = '⊙'  # Slider character at start
            progress_bar = f"{time_display} [{Colors.CYAN}{slider_char}{Colors.RESET}{'─' * empty_width}] {duration_display}"
        
        # Center the progress bar
        padding = width - len(progress_bar) - 2 + len(Colors.CYAN) + len(Colors.RESET)  # Adjust for color codes
        left_pad = padding // 2
        right_pad = padding - left_pad
        progress_line = "║" + " " * left_pad + progress_bar + " " * right_pad + "║"
        
        frame[progress_pos] = progress_line
    
    # Playback state and time info
    state_color = {
        "playing": Colors.GREEN,
        "paused": Colors.YELLOW,
        "stopped": Colors.RED,
        "error": Colors.RED,
        "unknown": Colors.WHITE
    }.get(info["state"], Colors.WHITE)
    
    state_text = info["state"].upper()
    
    state_line = center_text(state_text, width)
    frame[state_pos] = state_color + state_line + Colors.RESET
    
    # Connection status indicator in left part of the bottom line
    if info["state"] == "error":
        status_text = f"{Colors.RED}●{Colors.RESET} Connection Error"
    else:
        status_text = f"{Colors.GREEN}●{Colors.RESET} Connected"
    help_text = f"Press {Colors.YELLOW}Ctrl+C{Colors.RESET} to exit"

    # Helper to strip ANSI codes for length calculation
    import re
    def strip_ansi(s):
        return re.sub(r'\x1b\[[0-9;]*m', '', s)

    visible_left = len(strip_ansi(status_text))
    visible_right = len(strip_ansi(help_text))
    content_width = width - 4  # 2 for borders, 2 for spaces
    middle_padding = content_width - visible_left - visible_right
    if middle_padding < 1:
        middle_padding = 1
    bottom_line = f"║ {status_text}{' ' * middle_padding}{help_text} ║"
    frame[-2] = bottom_line
    
    return frame


def clear_terminal():
    """Clear the terminal screen."""
    os.system('cls' if os.name == 'nt' else 'clear')


def main():
    """Main function to run the now playing display."""
    args = parse_arguments()
    api_base_url = args.url
    update_interval = args.interval
    
    # Parse custom size if provided
    custom_width = None
    custom_height = None
    if args.size:
        try:
            size_parts = args.size.lower().split('x')
            if len(size_parts) == 2:
                custom_width = int(size_parts[0])
                custom_height = int(size_parts[1])
                print(f"Using custom size: {custom_width}x{custom_height}")
        except ValueError:
            print(f"Invalid size format '{args.size}'. Using terminal size instead.")
    
    print(f"HiFiBerry AudioControl - Now Playing Display")
    print(f"API URL: {api_base_url}")
    print(f"Update interval: {update_interval} seconds")
    print("Press Ctrl+C to exit")
    
    # Auto-retry connection if it fails initially
    retry_count = 0
    max_retries = 5
    connected = False
    
    while not connected and retry_count < max_retries:
        test_data = fetch_now_playing(api_base_url)
        if "error" not in test_data:
            connected = True
            break
        
        retry_count += 1
        if retry_count < max_retries:
            print(f"Connection failed. Retrying ({retry_count}/{max_retries})...")
            time.sleep(2)  # Wait before retry
    
    # Continue regardless of connection success (it will show error screen if needed)
    time.sleep(1)  # Give user time to read the initial message
    
    try:
        while True:
            # Get terminal size or use custom size if specified
            if custom_width and custom_height:
                terminal_width = custom_width
                terminal_height = custom_height
            else:
                try:
                    # On Windows, use os.get_terminal_size()
                    terminal_width = os.get_terminal_size().columns
                    terminal_height = os.get_terminal_size().lines
                except (AttributeError, OSError, IOError):
                    # Try to get size through stty command on Unix-like systems
                    try:
                        import subprocess
                        result = subprocess.run(['stty', 'size'], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=False)
                        if result.returncode == 0 and result.stdout.strip():
                            size_parts = result.stdout.strip().split()
                            terminal_height = int(size_parts[0])
                            terminal_width = int(size_parts[1])
                        else:
                            raise ValueError("Failed to get terminal size")
                    except (ImportError, ValueError, IndexError, FileNotFoundError):
                        # Fallback if we can't get terminal size
                        terminal_width = 80
                        terminal_height = 24
            
            # Fetch and format data
            now_playing_data = fetch_now_playing(api_base_url)
            formatted_data = format_now_playing(now_playing_data)
            
            # Create and update frame to fill the terminal
            frame = create_frame(terminal_width, terminal_height)
            frame = update_frame(frame, formatted_data, terminal_width, terminal_height)
            
            # Clear screen and display frame
            clear_terminal()
            print("\n".join(frame))
            
            # Wait for next update
            time.sleep(update_interval)
    except KeyboardInterrupt:
        print("\nExiting...")


if __name__ == "__main__":
    main()
