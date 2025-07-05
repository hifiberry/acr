/**
 * Audio Control REST (ACR) Player Functions
 * 
 * A collection of reusable functions for interacting with the ACR API
 * to control audio playback across different web interfaces.
 */

// Configuration
const PLAYER_CONFIG = {
    pollingInterval: 30000, // Time in milliseconds between updates (30 seconds)
    fastUpdateAfterCommand: 300, // Time to wait for quick update after sending a command
    wsReconnectInterval: 5000,  // Time to wait before attempting to reconnect WebSocket
    progressUpdateInterval: 500,  // Time in milliseconds between progress bar updates (0.5 seconds)
    apiBasePath: '/api'
};

// Default player capabilities (all disabled)
const DEFAULT_CAPABILITIES = {
    canPlay: false,
    canPause: false,
    canStop: false,
    canPrevious: false,
    canNext: false,
    canSeek: false,
    hasQueue: false,
    canShuffle: false,
    canLoop: false
};

/**
 * Extract player capabilities from response data
 * @param {Object} data - The player data object
 * @returns {Object} The player capabilities
 */
function extractPlayerCapabilities(data) {
    const capabilities = { ...DEFAULT_CAPABILITIES };
    
    if (!data || !data.player) {
        return capabilities;
    }
    
    // Check if player metadata contains capabilities information
    if (data.player.metadata && data.player.metadata.capabilities) {
        const caps = data.player.metadata.capabilities;
        capabilities.canPlay = caps.includes('play');
        capabilities.canPause = caps.includes('pause');
        capabilities.canStop = caps.includes('stop');
        capabilities.canPrevious = caps.includes('previous');
        capabilities.canNext = caps.includes('next');
        capabilities.canSeek = caps.includes('seek');
        capabilities.hasQueue = caps.includes('queue');
        capabilities.canShuffle = caps.includes('shuffle') || caps.includes('random');
        capabilities.canLoop = caps.includes('loop');
        return capabilities;
    }
    
    // Alternative: Check if capabilities are directly in player info
    if (data.player.capabilities) {
        const caps = data.player.capabilities;
        capabilities.canPlay = caps.includes('play');
        capabilities.canPause = caps.includes('pause');
        capabilities.canStop = caps.includes('stop');
        capabilities.canPrevious = caps.includes('previous');
        capabilities.canNext = caps.includes('next');
        capabilities.canSeek = caps.includes('seek');
        capabilities.hasQueue = caps.includes('queue');
        capabilities.canShuffle = caps.includes('shuffle') || caps.includes('random');
        capabilities.canLoop = caps.includes('loop');
        return capabilities;
    }
    
    // Fallback: If no explicit capabilities, infer from player state
    // Most players will support basic operations
    if (data.player.is_active) {
        capabilities.canPlay = true;
        capabilities.canPause = true;
        capabilities.canStop = true;
        
        // Assume track navigation if a song is playing
        if (data.song) {
            capabilities.canPrevious = true;
            capabilities.canNext = true;
        }
        
        // If player has shuffle or loop state, assume it can change these
        if (typeof data.shuffle === 'boolean') {
            capabilities.canShuffle = true;
        }
        
        if (data.loop_mode) {
            capabilities.canLoop = true;
        }
        
        // If position is reported, assume seeking is possible
        if (data.position !== undefined && data.position !== null) {
            capabilities.canSeek = true;
        }
        
        // If the player has a library, assume it also has a queue
        if (data.player.has_library) {
            capabilities.hasQueue = true;
        }
    }
    
    return capabilities;
}

/**
 * Format time in seconds to MM:SS or HH:MM:SS
 * @param {number} seconds - Time in seconds
 * @returns {string} Formatted time string
 */
function formatTime(seconds) {
    if (seconds === undefined || seconds === null) return '00:00';
    
    seconds = Math.floor(seconds);
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const secs = seconds % 60;
    
    if (hours > 0) {
        return `${hours.toString().padStart(2, '0')}:${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
    } else {
        return `${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
    }
}

/**
 * Fetch available players from the API
 * @param {string} apiBase - The base URL for the API
 * @returns {Promise<Array>} Array of player objects
 */
async function fetchPlayers(apiBase = PLAYER_CONFIG.apiBasePath) {
    try {
        const response = await fetch(`${apiBase}/players`);
        const data = await response.json();
        
        if (data.players && Array.isArray(data.players)) {
            return data.players;
        } else {
            console.error('Invalid players data structure:', data);
            return [];
        }
    } catch (error) {
        console.error('Failed to fetch players:', error);
        return [];
    }
}

/**
 * Fetch current player and now playing information
 * @param {string} apiBase - The base URL for the API
 * @returns {Promise<Object>} Current player data
 */
async function fetchCurrentPlayer(apiBase = PLAYER_CONFIG.apiBasePath) {
    try {
        const response = await fetch(`${apiBase}/now-playing`);
        return await response.json();
    } catch (error) {
        console.error('Failed to fetch current player:', error);
        return null;
    }
}

/**
 * Fetch queue information for the current player
 * @param {string} playerName - Optional specific player name to fetch queue for
 * @param {string} apiBase - The base URL for the API
 * @returns {Promise<Array>} Array of queue items
 */
async function fetchQueue(playerName = null, apiBase = PLAYER_CONFIG.apiBasePath) {
    try {
        let url;
        if (playerName) {
            url = `${apiBase}/player/${playerName}/queue`;
        } else {
            url = `${apiBase}/player/active/queue`;
        }
        
        const response = await fetch(url);
        
        if (response.ok) {
            const data = await response.json();
            return data.queue || [];
        } else {
            console.error('Failed to fetch queue:', response.statusText);
            return [];
        }
    } catch (error) {
        console.error('Error fetching queue:', error);
        return [];
    }
}

/**
 * Send a command to the player
 * @param {string} command - The command to send
 * @param {string} playerName - Optional specific player name
 * @param {string} apiBase - The base URL for the API
 * @returns {Promise<boolean>} Success or failure
 */
async function sendCommand(command, playerName = null, apiBase = PLAYER_CONFIG.apiBasePath) {
    try {
        // Build the URL based on whether we're using a specific player or the active player
        let url;
        if (playerName) {
            // Send to specific player
            url = `${apiBase}/player/${playerName}/command/${command}`;
        } else {
            // Send to active player (default)
            url = `${apiBase}/player/active/command/${command}`;
        }
        
        console.log(`Sending command to: ${url}`);
        const response = await fetch(url, {
            method: 'POST'
        });
        
        return response.ok;
    } catch (error) {
        console.error('Error sending command:', error);
        return false;
    }
}

/**
 * Toggle play/pause based on current state
 * @param {Object} currentData - The current player data
 * @param {string} playerName - Optional specific player name
 * @param {string} apiBase - The base URL for the API
 * @returns {Promise<boolean>}
 */
async function togglePlayPause(currentData, playerName = null, apiBase = PLAYER_CONFIG.apiBasePath) {
    if (!currentData) return false;
    
    // Check state case-insensitively to handle different API response formats
    const isPlaying = currentData.state && 
                      currentData.state.toLowerCase() === 'playing';
    
    const command = isPlaying ? 'pause' : 'play';
    console.log(`Sending command: ${command} based on state: ${currentData.state}`);
    return await sendCommand(command, playerName, apiBase);
}

/**
 * Send a seek command to the player
 * @param {number} position - The position to seek to in seconds
 * @param {string} playerName - Optional specific player name
 * @param {string} apiBase - The base URL for the API
 * @returns {Promise<boolean>} Success or failure
 */
async function seekToPosition(position, playerName = null, apiBase = PLAYER_CONFIG.apiBasePath) {
    try {
        const seekCommand = `seek:${Math.floor(position)}`;
        return await sendCommand(seekCommand, playerName, apiBase);
    } catch (error) {
        console.error('Error seeking to position:', error);
        return false;
    }
}

/**
 * Cycle through loop modes: None -> Track -> Playlist -> None
 * @param {Object} currentData - The current player data
 * @param {string} playerName - Optional specific player name
 * @param {string} apiBase - The base URL for the API
 * @returns {Promise<boolean>} Success or failure
 */
async function cycleLoopMode(currentData, playerName = null, apiBase = PLAYER_CONFIG.apiBasePath) {
    if (!currentData) return false;
    
    let nextMode;
    // Check the loop mode value case-insensitively since API might use different cases
    const currentMode = (currentData.loop_mode || '').toLowerCase();
    
    console.log(`Current loop mode: ${currentMode}`);
    
    switch(currentMode) {
        case 'none':
        case 'no':
            nextMode = 'track';
            break;
        case 'track':
        case 'song':
            nextMode = 'playlist';
            break;
        case 'playlist':
        default:
            nextMode = 'none';
            break;
    }
    
    console.log(`Setting new loop mode: ${nextMode}`);
    return await sendCommand(`set_loop:${nextMode}`, playerName, apiBase);
}

/**
 * Toggle shuffle state
 * @param {string} playerName - Optional specific player name
 * @param {string} apiBase - The base URL for the API
 * @returns {Promise<boolean>} Success or failure
 */
async function toggleShuffle(playerName = null, apiBase = PLAYER_CONFIG.apiBasePath) {
    try {
        // First fetch the current state to toggle it
        const response = await fetch(`${apiBase}/now-playing`);
        const data = await response.json();
        
        if (data.shuffle !== undefined) {
            // Send the opposite of the current shuffle state
            return await sendCommand(`set_random:${!data.shuffle}`, playerName, apiBase);
        }
        return false;
    } catch (error) {
        console.error('Error toggling shuffle:', error);
        return false;
    }
}

/**
 * Clear the player's queue
 * @param {string} playerName - Optional specific player name
 * @param {string} apiBase - The base URL for the API
 * @returns {Promise<boolean>} Success or failure
 */
async function clearQueue(playerName = null, apiBase = PLAYER_CONFIG.apiBasePath) {
    return await sendCommand('clear_queue', playerName, apiBase);
}

/**
 * Update song info in the UI
 * @param {Object} song - The song data object
 * @param {Element} nowPlayingInfoEl - The element to display song info
 * @param {Element} songThumbnailEl - The element for song thumbnail
 * @param {Element} noThumbnailEl - The element to display when no thumbnail is available
 * @param {Element} progressBarEl - The progress bar element
 * @param {Element} songAlbumEl - The element to display album info
 * @param {Element} songTrackNumberEl - The element to display track number
 * @param {Element} songLikedButtonEl - The button to display liked status
 */
function updateSongInfo(song, nowPlayingInfoEl, songThumbnailEl, noThumbnailEl, progressBarEl, songAlbumEl, songTrackNumberEl, songLikedButtonEl) {
    if (song && song.title) {
        let displayText = `<strong>${song.title}</strong>`;
        if (song.artist) {
            displayText += ` by ${song.artist}`;
        }
        nowPlayingInfoEl.innerHTML = displayText;

        // Update additional details
        songAlbumEl.textContent = song.album || 'N/A';
        songTrackNumberEl.textContent = song.track_number || 'N/A';

        // Update liked status button
        if (song.liked === true) {
            songLikedButtonEl.textContent = '♥'; // Filled heart
            songLikedButtonEl.title = 'Liked';
        } else if (song.liked === false) {
            songLikedButtonEl.textContent = '♡'; // Empty heart
            songLikedButtonEl.title = 'Not Liked';
        } else {
            songLikedButtonEl.textContent = '?'; // Unknown, or could be an empty heart too
            songLikedButtonEl.title = 'Liked status unknown';
        }

        // Update album art
        if (song.cover_art_url) {
            songThumbnailEl.src = song.cover_art_url;
            songThumbnailEl.style.display = 'inline-block';
            noThumbnailEl.style.display = 'none';
        } else if (song.thumbnail) { // Fallback to thumbnail if cover_art_url is not available
            songThumbnailEl.src = song.thumbnail;
            songThumbnailEl.style.display = 'inline-block';
            noThumbnailEl.style.display = 'none';
        } else {
            songThumbnailEl.style.display = 'none';
            noThumbnailEl.style.display = 'inline-flex';
        }
        
        // Update progress bar if duration and position are available
        if (song.duration && song.position !== undefined) {
            const percentage = song.duration > 0 ? (song.position / song.duration) * 100 : 0;
            progressBarEl.style.width = `${percentage}%`;
        } else {
            progressBarEl.style.width = '0%';
        }
    } else {
        nowPlayingInfoEl.textContent = 'Nothing playing';
        songThumbnailEl.style.display = 'none';
        noThumbnailEl.style.display = 'inline-flex';
        progressBarEl.style.width = '0%';
        // Reset additional details
        songAlbumEl.textContent = 'N/A';
        songTrackNumberEl.textContent = 'N/A';
        songLikedButtonEl.textContent = '?';
        songLikedButtonEl.title = 'Liked status unknown';
    }
}

/**
 * Update player info display
 * @param {Object} data - The player data
 * @param {Element} currentPlayerInfoEl - The element to update with player info
 * @param {Element} libraryBtnEl - Optional library button element to update
 */
function updatePlayerInfo(data, currentPlayerInfoEl, libraryBtnEl = null) {
    if (data.player && data.player.name) {
        const stateClass = `state-${data.state.toLowerCase()}`;
        currentPlayerInfoEl.innerHTML = `
            <div>
                <span class="state-indicator ${stateClass}"></span>
                <strong>${data.player.name}</strong> (${data.state})
            </div>
        `;

        // Update Library button link if player has a library
        if (libraryBtnEl) {
            const hasLibrary = data.player.has_library;
            libraryBtnEl.disabled = !hasLibrary;
            if (hasLibrary) {
                libraryBtnEl.href = `library.html?player=${encodeURIComponent(data.player.name)}`;
            }
        }
    } else {
        currentPlayerInfoEl.textContent = 'No active player';
        if (libraryBtnEl) {
            libraryBtnEl.disabled = true;
        }
    }
}

/**
 * Update now playing information
 * @param {Object} data - The player data
 * @param {Element} nowPlayingInfoEl - The element to display now playing info
 * @param {Element} progressBarEl - The progress bar element
 * @param {Element} songThumbnailEl - The element for song thumbnail
 * @param {Element} noThumbnailEl - The element to display when no thumbnail is available
 * @param {function} startAutoProgressFn - Optional function to start auto progress updates
 * @param {function} stopAutoProgressFn - Optional function to stop auto progress updates
 */
function updateNowPlaying(data, nowPlayingInfoEl, progressBarEl, songThumbnailEl, noThumbnailEl, startAutoProgressFn = null, stopAutoProgressFn = null) {
    if (data.song) {
        const song = data.song;
        const position = data.position ? formatTime(data.position) : '0:00';
        const duration = song.duration ? formatTime(song.duration) : '0:00';
        
        // Update progress bar and position text
        updateProgress(data.song, data.position, progressBarEl, nowPlayingInfoEl);
        nowPlayingInfoEl.innerHTML = `
            <div><strong>Title:</strong> ${song.title || 'Unknown'}</div>
            <div><strong>Artist:</strong> ${song.artist || 'Unknown'}</div>
            ${song.album ? `<div><strong>Album:</strong> ${song.album}</div>` : ''}
            <div><strong>Position:</strong> ${position} / ${duration}</div>
        `;

        // Update song thumbnail - prioritize cover_art_url over thumbnail
        if (song.cover_art_url) {
            console.log("Using cover_art_url for album art:", song.cover_art_url);
            songThumbnailEl.src = song.cover_art_url;
            songThumbnailEl.style.display = 'inline-block';
            noThumbnailEl.style.display = 'none';
        } else if (song.thumbnail) {
            console.log("Using thumbnail for album art:", song.thumbnail);
            songThumbnailEl.src = song.thumbnail;
            songThumbnailEl.style.display = 'inline-block';
            noThumbnailEl.style.display = 'none';
        } else {
            console.log("No album art available");
            songThumbnailEl.style.display = 'none';
            noThumbnailEl.style.display = 'inline-flex';
        }
        
        // Start auto progress updates if the player is playing
        if (startAutoProgressFn && stopAutoProgressFn) {
            if (data.state && data.state.toLowerCase() === 'playing') {
                startAutoProgressFn();
            } else {
                stopAutoProgressFn();
            }
        }
    } else {
        nowPlayingInfoEl.textContent = 'Nothing playing';
        progressBarEl.style.width = '0%';
        songThumbnailEl.style.display = 'none';
        noThumbnailEl.style.display = 'inline-flex';
        if (stopAutoProgressFn) {
            stopAutoProgressFn();
        }
    }
}

/**
 * Update control buttons based on available capabilities and current state
 * @param {Object} data - The player data
 * @param {Object} playerCapabilities - The player capabilities object
 * @param {Object} buttons - Object containing button elements
 * @param {Element} buttons.playPauseBtn - Play/Pause button element
 * @param {Element} buttons.stopBtn - Stop button element
 * @param {Element} buttons.prevBtn - Previous button element
 * @param {Element} buttons.nextBtn - Next button element
 * @param {Element} buttons.loopModeBtn - Loop mode button element
 * @param {Element} buttons.toggleShuffleBtn - Toggle shuffle button element
 * @param {Element} buttons.libraryBtn - Library button element
 * @param {Element} queuePanelEl - Queue panel element
 * @param {Element} queueContainerEl - Queue container element
 * @param {function} fetchQueueFn - Function to fetch the queue
 */
function updateControlButtons(data, playerCapabilities, buttons, queuePanelEl = null, queueContainerEl = null, fetchQueueFn = null) {
    // Check playback state
    const isPlaying = data.state && data.state.toLowerCase() === 'playing';
    const isPaused = data.state && data.state.toLowerCase() === 'paused'; 
    const isPlayingOrPaused = isPlaying || isPaused;
    
    // Enable/disable buttons based on player capabilities
    buttons.playPauseBtn.disabled = !(playerCapabilities.canPlay || playerCapabilities.canPause);
    buttons.stopBtn.disabled = !playerCapabilities.canStop || !isPlayingOrPaused;
    buttons.prevBtn.disabled = !playerCapabilities.canPrevious;
    buttons.nextBtn.disabled = !playerCapabilities.canNext;
    buttons.loopModeBtn.disabled = !playerCapabilities.canLoop;
    buttons.toggleShuffleBtn.disabled = !playerCapabilities.canShuffle;
    
    // Enable/disable Library button based on whether player has a library
    const hasLibrary = data.player && data.player.has_library;
    buttons.libraryBtn.disabled = !hasLibrary;
    
    // Show or hide queue panel based on whether player supports queue
    if (queuePanelEl && queueContainerEl && fetchQueueFn) {
        if (playerCapabilities.hasQueue) {
            queuePanelEl.style.display = 'block';
            // Fetch queue if it's now visible and we haven't already
            if (!queueContainerEl.hasAttribute('data-loaded')) {
                fetchQueueFn();
            }
        } else {
            queuePanelEl.style.display = 'none';
        }
    }
    
    // Update play/pause button icon based on state
    if (buttons.playPauseBtn.disabled === false) {
        // Update play/pause button icon
        if (isPlaying) {
            buttons.playPauseBtn.innerHTML = '<i class="fas fa-pause"></i>';
            buttons.playPauseBtn.title = 'Pause';
        } else {
            buttons.playPauseBtn.innerHTML = '<i class="fas fa-play"></i>';
            buttons.playPauseBtn.title = 'Play';
        }
    }
    
    if (buttons.loopModeBtn.disabled === false) {
        // Update loop mode button
        let loopIcon, loopText;
        const loopMode = (data.loop_mode || '').toLowerCase();
        
        switch(loopMode) {
            case 'track':
            case 'song':
                loopIcon = 'fa-redo';
                loopText = 'Loop: Track';
                break;
            case 'playlist':
                loopIcon = 'fa-retweet';
                loopText = 'Loop: Playlist';
                break;
            case 'none':
            case 'no':
            default:
                loopIcon = 'fa-times';
                loopText = 'Loop: None';
                break;
        }
        buttons.loopModeBtn.innerHTML = `<i class="fas ${loopIcon}"></i> ${loopText}`;
        buttons.loopModeBtn.classList.toggle('active', loopMode !== 'none' && loopMode !== 'no');
    }
    
    if (buttons.toggleShuffleBtn.disabled === false) {
        // Update shuffle button state
        buttons.toggleShuffleBtn.classList.toggle('active', data.shuffle);
        buttons.toggleShuffleBtn.innerHTML = `<i class="fas fa-random"></i> Shuffle: ${data.shuffle ? 'On' : 'Off'}`;
    }
    
    // Log the current capabilities
    console.log('Player capabilities:', playerCapabilities);
}

/**
 * Setup progress bar click handler for seeking
 * @param {Element} progressContainerEl - The progress container element
 * @param {function} seekPositionFn - Function to seek to a position
 * @param {Object} currentData - The current player data
 * @param {Object} playerCapabilities - The player capabilities
 */
function setupProgressBarClickHandler(progressContainerEl, seekPositionFn, currentData, playerCapabilities) {
    progressContainerEl.addEventListener('click', function(event) {
        // Only allow seeking if we have a song playing and the player supports seeking
        if (!currentData || !currentData.song || !currentData.song.duration || !playerCapabilities.canSeek) {
            return;
        }
        
        // Calculate the position to seek to based on the click position
        const rect = this.getBoundingClientRect();
        const clickOffset = event.clientX - rect.left;
        const clickPercentage = clickOffset / rect.width;
        const seekPosition = currentData.song.duration * clickPercentage;
        
        // Send the seek command
        console.log(`Seeking to position: ${formatTime(seekPosition)} (${Math.round(clickPercentage * 100)}%)`);
        seekPositionFn(seekPosition);
    });
}

/**
 * Update player dropdown with available players
 * @param {Array} players - Array of player objects
 * @param {Element} playerSelectEl - The player select dropdown element
 * @param {Element} libraryBtnEl - Optional library button element to update
 * @param {function} setCurrentPlayerNameFn - Function to set the current player name
 */
function updatePlayerDropdown(players, playerSelectEl, libraryBtnEl = null, setCurrentPlayerNameFn = null) {
    playerSelectEl.innerHTML = '';
    
    if (players.length === 0) {
        const option = document.createElement('option');
        option.value = '';
        option.textContent = 'No players available';
        playerSelectEl.appendChild(option);
        return;
    }
    
    // Add a default option for the active player
    const defaultOption = document.createElement('option');
    defaultOption.value = '';
    defaultOption.textContent = 'Default (Active Player)';
    playerSelectEl.appendChild(defaultOption);
    
    // Find the active player for library button state
    const activePlayer = players.find(player => player.is_active);
    if (activePlayer) {
        // Update library button state based on active player
        if (libraryBtnEl) {
            const hasLibrary = activePlayer.has_library;
            libraryBtnEl.disabled = !hasLibrary;
        }
        
        // Mark the default option as selected
        defaultOption.selected = true;
        if (setCurrentPlayerNameFn) {
            setCurrentPlayerNameFn(null); // Use the default active player
        }
    }
    
    // Add remaining players to dropdown
    players.forEach(player => {
        const option = document.createElement('option');
        option.value = player.name;
        option.textContent = player.name;
        playerSelectEl.appendChild(option);
    });
}

/**
 * Open player library in a new window/tab
 * @param {string} playerName - The player name
 */
function openPlayerLibrary(playerName = 'active') {
    const libraryUrl = `library.html?player=${encodeURIComponent(playerName)}`;
    console.log(`Navigating to library: ${libraryUrl}`);
    window.location.href = libraryUrl;
}

/**
 * Display queue data in the UI
 * @param {Object} data - The queue data
 * @param {Array} data.queue - Array of queue items
 * @param {Element} queueContainerEl - The queue container element
 * @param {Element} queueCountEl - The queue count element
 * @param {Object} currentData - The current player data
 * @param {function} playQueueIndexFn - Function to play a queue item by index
 * @param {function} removeTrackFromQueueFn - Function to remove a track from the queue
 */
function displayQueue(data, queueContainerEl, queueCountEl, currentData, playQueueIndexFn, removeTrackFromQueueFn) {
    // Validate data
    if (!data || !data.queue || !Array.isArray(data.queue)) {
        queueContainerEl.innerHTML = '<div class="queue-message">No queue items</div>';
        queueCountEl.textContent = '(0)';
        return;
    }
    
    // Update queue count badge
    queueCountEl.textContent = `(${data.queue.length})`;
    
    // If queue is empty
    if (data.queue.length === 0) {
        queueContainerEl.innerHTML = '<div class="queue-message">Queue is empty</div>';
        return;
    }
    
    // Build queue items
    let queueHtml = '';
    
    data.queue.forEach((track, index) => {
        // Check if this is the currently playing track
        const isCurrent = currentData && currentData.song && 
                          currentData.song.uri === track.uri;
        
        const trackTitle = track.name || track.title || 'Unknown';
        const trackArtist = track.artist || 'Unknown Artist';
        
        queueHtml += `
            <div class="queue-item ${isCurrent ? 'current' : ''}" data-uri="${track.uri || ''}" data-position="${index}" data-index="${index}">
                <div class="queue-item-info">
                    <div class="queue-item-title">${isCurrent ? '▶ ' : ''}${trackTitle}</div>
                    <div class="queue-item-artist">${trackArtist}</div>
                </div>
                <div class="queue-item-actions">
                    <button class="queue-action-btn play-track-btn" title="Play Now" data-index="${index}">
                        <i class="fas fa-play"></i>
                    </button>
                    <button class="queue-action-btn remove-track-btn" title="Remove" data-position="${index}">
                        <i class="fas fa-times"></i>
                    </button>
                </div>
            </div>
        `;
    });
    
    // Set HTML content
    queueContainerEl.innerHTML = queueHtml;
    
    // Add event listeners to the queue action buttons
    const playButtons = queueContainerEl.querySelectorAll('.play-track-btn');
    const removeButtons = queueContainerEl.querySelectorAll('.remove-track-btn');
    
    // Play track buttons
    playButtons.forEach(button => {
        button.addEventListener('click', function() {
            const index = parseInt(this.getAttribute('data-index'), 10);
            if (!isNaN(index)) {
                playQueueIndexFn(index);
            }
        });
    });
    
    // Remove track buttons
    removeButtons.forEach(button => {
        button.addEventListener('click', function() {
            const trackPosition = this.getAttribute('data-position');
            if (trackPosition) {
                removeTrackFromQueueFn(trackPosition);
            }
        });
    });
}

/**
 * Play a track from the queue by its index
 * @param {number} index - The index of the track in the queue
 * @param {string} playerName - Optional specific player name
 * @param {string} apiBase - The base URL for the API
 * @returns {Promise<boolean>} Success or failure
 */
async function playQueueIndex(index, playerName = null, apiBase = PLAYER_CONFIG.apiBasePath) {
    if (index === undefined || index === null) return false;
    
    console.log(`Playing track at index: ${index} in queue`);
    return await sendCommand(`play_queue_index:${index}`, playerName, apiBase);
}

/**
 * Remove a track from the queue
 * @param {number} position - The position of the track in the queue
 * @param {string} playerName - Optional specific player name
 * @param {string} apiBase - The base URL for the API
 * @returns {Promise<boolean>} Success or failure
 */
async function removeTrackFromQueue(position, playerName = null, apiBase = PLAYER_CONFIG.apiBasePath) {
    if (position === undefined || position === null) return false;
    
    console.log(`Removing track at position: ${position} from queue`);
    return await sendCommand(`remove_track:${position}`, playerName, apiBase);
}

/**
 * Toggle the like status of the current song
 * @param {string} playerName - Optional specific player name
 * @param {Object} currentData - The current player data with song information
 * @param {string} apiBase - The base URL for the API
 * @returns {Promise<boolean>} Success or failure
 */
async function toggleLike(playerName = null, currentData = null, apiBase = PLAYER_CONFIG.apiBasePath) {
    if (!currentData || !currentData.song || !currentData.player) {
        console.error('No song is currently playing');
        return false;
    }
    
    // Get the current song and toggle the liked status
    const song = currentData.song;
    const newLikedStatus = !(song.liked === true);
    console.log(`Toggling like status for "${song.title}" to ${newLikedStatus}`);
    
    // Get the player name to use
    let playerNameToUse = playerName;
    if (!playerNameToUse) {
        try {
            playerNameToUse = await change_active_player(apiBase);
        } catch (error) {
            console.error('Error getting active player name:', error);
            playerNameToUse = 'active'; // Fallback on error
        }
    }
    
    try {
        // Create the updated song object with only the liked status changed
        const updatedSong = {
            liked: newLikedStatus
        };
        
        // Send the update to the API
        const url = `${apiBase}/player/${playerNameToUse}/song/update`;
        const response = await fetch(url, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json'
            },
            body: JSON.stringify(updatedSong)
        });
        
        if (response.ok) {
            // Update the local data immediately for better UI responsiveness
            if (currentData && currentData.song) {
                currentData.song.liked = newLikedStatus;
            }
            return true;
        } else {
            console.error('Failed to update like status:', response.statusText);
            return false;
        }
    } catch (error) {
        console.error('Error toggling like status:', error);
        return false;
    }
}

/**
 * Create a WebSocket connection for player events
 * @param {Object} options - The WebSocket options
 * @param {string} options.hostname - The hostname for the WebSocket connection
 * @param {number} options.port - The port for the WebSocket connection
 * @param {function} options.onConnect - Callback when WebSocket connects
 * @param {function} options.onDisconnect - Callback when WebSocket disconnects
 * @param {function} options.onMessage - Callback when WebSocket receives a message
 * @param {function} options.onError - Callback when WebSocket encounters an error
 * @param {Element} options.statusIndicator - Element to display connection status indicator
 * @param {Element} options.statusText - Element to display connection status text
 * @returns {Object} WebSocket controller object
 */
function createPlayerWebSocket(options) {
    let socket = null;
    let reconnectTimer = null;
    const wsUrl = `ws://${options.hostname}:${options.port}/api/events`;
    
    // Initialize the status UI
    if (options.statusIndicator) {
        options.statusIndicator.style.backgroundColor = '#6c757d'; // gray
    }
    if (options.statusText) {
        options.statusText.textContent = 'Disconnected';
    }
    
    // Update status UI based on connection state
    const updateStatusUI = (connected) => {
        if (options.statusIndicator) {
            options.statusIndicator.style.backgroundColor = connected ? '#28a745' : '#dc3545'; // green : red
        }
        if (options.statusText) {
            options.statusText.textContent = connected ? 'Connected' : 'Disconnected';
        }
    };
    
    // Connect to WebSocket
    const connect = () => {
        if (socket) {
            return; // Already connected or connecting
        }
        
        try {
            // Clear any pending reconnect timer
            if (reconnectTimer) {
                clearTimeout(reconnectTimer);
                reconnectTimer = null;
            }
            
            socket = new WebSocket(wsUrl);
            
            socket.onopen = () => {
                console.log('WebSocket connected');
                updateStatusUI(true);
                if (options.onConnect) {
                    options.onConnect();
                }
            };
            
            socket.onclose = (event) => {
                console.log(`WebSocket closed (code: ${event.code}, reason: ${event.reason || 'none'})`);
                updateStatusUI(false);
                
                // Call disconnect callback
                if (options.onDisconnect) {
                    options.onDisconnect(event);
                }
                
                // Schedule reconnect
                socket = null;
                if (reconnectTimer) {
                    clearTimeout(reconnectTimer);
                }
                reconnectTimer = setTimeout(connect, PLAYER_CONFIG.wsReconnectInterval);
            };
            
            socket.onerror = (error) => {
                console.error('WebSocket error:', error);
                if (options.onError) {
                    options.onError(error);
                }
            };
              socket.onmessage = (event) => {
                try {
                    const data = JSON.parse(event.data);
                    console.log('WebSocket message received:', data);
                    
                    // Handle welcome message and subscription updates
                    if (data.type === 'welcome' || data.type === 'subscription_updated') {
                        console.log(`WebSocket ${data.type} message:`, data.message);
                        return;
                    }
                    
                    if (options.onMessage) {
                        options.onMessage(data);
                    }
                } catch (error) {
                    console.error('Error parsing WebSocket message:', error);
                }
            };
        } catch (error) {
            console.error('Failed to connect WebSocket:', error);
            if (options.onError) {
                options.onError(error);
            }
            // Schedule reconnect after error
            if (reconnectTimer) {
                clearTimeout(reconnectTimer);
            }
            reconnectTimer = setTimeout(connect, PLAYER_CONFIG.wsReconnectInterval);
        }
    };
    
    // Disconnect from WebSocket
    const disconnect = () => {
        if (reconnectTimer) {
            clearTimeout(reconnectTimer);
            reconnectTimer = null;
        }
        
        if (socket) {
            socket.close();
            socket = null;
        }
        
        updateStatusUI(false);
    };
    
    // Get the socket object
    const getSocket = () => socket;
      // Return controller object with public methods
    return {
        connect,
        disconnect,
        getSocket,
        updateSubscription: (subscription) => {
            if (socket && socket.readyState === WebSocket.OPEN) {
                socket.send(JSON.stringify(subscription));
                return true;
            }
            return false;
        },
        subscribe: (playerName, eventTypes) => {
            if (socket && socket.readyState === WebSocket.OPEN) {
                // Create subscription object
                const subscription = {
                    players: playerName ? [playerName] : null,
                    event_types: eventTypes && eventTypes.length > 0 ? eventTypes : null
                };
                
                // Send subscription
                socket.send(JSON.stringify(subscription));
                console.log(`Subscribed to player events: ${JSON.stringify(subscription)}`);
                return true;
            }
            return false;
        }
    };
}

/**
 * Handle player events received from WebSocket
 * @param {Object} data - The event data
 * @param {Object} options - Options for handling the event
 * @param {string} options.currentPlayerName - The current player name
 * @param {Object} options.currentData - The current player data
 * @param {function} options.fetchPlayers - Function to fetch players
 * @param {function} options.fetchCurrentPlayer - Function to fetch current player
 * @param {function} options.updatePlayerInfo - Function to update player info
 * @param {function} options.updateNowPlaying - Function to update now playing info
 * @param {function} options.updateControlButtons - Function to update control buttons
 * @param {function} options.updateSongInfo - Function to update song info
 * @param {function} options.fetchQueue - Function to fetch the queue
 * @param {Object} options.playerCapabilities - The player capabilities
 */
function handlePlayerEvent(data, options) {
    const {
        currentPlayerName,
        currentData,
        fetchPlayers,
        fetchCurrentPlayer,
        updatePlayerInfo,
        updateNowPlaying,
        updateControlButtons,
        updateSongInfo,
        fetchQueue,
        playerCapabilities
    } = options;

    // Handle different API response formats
    let eventType, playerName, isActivePlayer, source;
    
    // Get the event type (could be in different formats)
    if (data.event_type) {
        // Camel case format (event_type key)
        eventType = data.event_type;
        source = data.source || {};
        playerName = source.player_name;
        isActivePlayer = source.is_active_player;
    } else if (data.type) {
        // Snake case format (type key from WebSocket)
        eventType = data.type;
        playerName = data.player_name;
        // For snake_case format, assume it's for the active player
        // unless explicitly specified
        isActivePlayer = data.is_active_player;
    } else {
        console.log('Unknown event format:', data);
        return;
    }
    
    // Map snake_case event types to camelCase
    if (eventType) {
        switch (eventType) {
            case 'state_changed': 
                eventType = 'StateChanged'; 
                if (data.state) {
                    data.state = data.state;
                }
                break;
            case 'song_changed': eventType = 'SongChanged'; break;
            case 'position_changed': eventType = 'PlaybackPosition'; break;
            case 'loop_mode_changed': 
                eventType = 'LoopModeChanged'; 
                if (data.mode) {
                    data.loop_mode = data.mode;
                }
                break;
            case 'random_changed':
            case 'shuffle_changed': 
                eventType = 'ShuffleChanged'; 
                if (data.enabled !== undefined) {
                    data.shuffle = data.enabled;
                }
                break;            case 'queue_changed': eventType = 'QueueChanged'; break;
            case 'capabilities_changed': eventType = 'CapabilitiesChanged'; break;
            case 'song_information_update': eventType = 'SongInformationUpdate'; break;
            case 'metadata_changed': eventType = 'MetadataChanged'; break;
        }
    }
      // Check if this event is for our current player
    // When currentPlayerName is null, we're using the "Default (Active Player)" option
    // In this case, we need to handle events from the active player
    const isForCurrentPlayer = 
        (!currentPlayerName && (isActivePlayer === true || data.is_active === true || data.is_active_player === true)) || // Event for active player
        (currentPlayerName && playerName === currentPlayerName); // Event for a specific player we are viewing

    // If we still can't determine if this event is for us, but we're using the active player,
    // just assume it's for us since "active" is no longer supported in the WebSocket subscription
    // and the server may not be sending the is_active flag
    const assumeActiveForDefaultSelection = !currentPlayerName && (!isActivePlayer && isActivePlayer !== false);

    console.log(`Event ${eventType} is for player ${playerName || 'unknown'}, is active: ${isActivePlayer}, current player is ${currentPlayerName || 'active'}. ${isForCurrentPlayer || assumeActiveForDefaultSelection ? 'Processing' : 'Ignoring'}.`);

    if (isForCurrentPlayer || assumeActiveForDefaultSelection) {
        // Update UI based on event type
        switch (eventType) {
            case 'PlayerChanged':
            case 'PlayerAdded':
            case 'PlayerRemoved':
                // Player list might have changed, or active player changed
                fetchPlayers(); // Refresh player dropdown
                // If the active player changed, or our selected player was removed, we might need to refresh now-playing
                fetchCurrentPlayer(); 
                break;
            case 'StateChanged':
                // Update playback state and related UI elements
                if (currentData) {
                    currentData.state = data.state;
                    // Potentially update position if included, though full fetch might be better
                    if (data.position !== undefined) currentData.position = data.position;
                    updateControlButtons(currentData);
                    updatePlayerInfo(currentData); // Update state display in player info
                    updateNowPlaying(currentData); // Update progress bar and play/pause icon
                } else {
                    fetchCurrentPlayer(); // Fetch if no current data
                }
                break;
            case 'SongChanged':
                // Update with new song information
                if (currentData) {
                    currentData.song = data.song;
                    currentData.position = data.position !== undefined ? data.position : 0;
                    // If loop/shuffle status is part of this event, update them too
                    if (data.loop_mode !== undefined) currentData.loop_mode = data.loop_mode;
                    if (data.shuffle !== undefined) currentData.shuffle = data.shuffle;
                    
                    updateNowPlaying(currentData);
                    updateSongInfo(currentData.song); // Make sure this function exists and is comprehensive
                    updateControlButtons(currentData); // Controls might change based on new song/state
                    if (playerCapabilities.hasQueue) fetchQueue(); // Refresh queue if song changes
                } else {
                    fetchCurrentPlayer(); // Fetch if no current data
                }
                break;
            case 'PlaybackPosition':
                // Update playback position
                if (currentData && currentData.song) {
                    currentData.position = data.position;
                    updateNowPlaying(currentData); // This updates the progress bar
                }
                break;
            case 'LoopModeChanged':
                if (currentData) {
                    currentData.loop_mode = data.loop_mode;
                    updateControlButtons(currentData); // Updates loop button
                }
                break;
            case 'ShuffleChanged':
                if (currentData) {
                    currentData.shuffle = data.shuffle;
                    updateControlButtons(currentData); // Updates shuffle button
                }
                break;            case 'QueueChanged':
                // Queue has changed, refresh it
                if (playerCapabilities.hasQueue) {
                    fetchQueue();
                }
                break;
            case 'SongInformationUpdate':
            case 'song_information_update':
                // Update song information (cover art, liked status, etc.) without changing the entire song
                if (currentData && currentData.song && data.song) {
                    console.log('Received song information update:', data.song);
                    
                    // Merge the updated song information with the existing song object
                    Object.assign(currentData.song, data.song);
                    
                    // Update song info in the UI
                    updateSongInfo(currentData.song);
                    
                    // Also update the now playing display as it might include artwork
                    updateNowPlaying(currentData);
                }
                break;
            case 'MetadataChanged':
                // Handle metadata changes similarly to song information updates
                if (currentData && currentData.song && data.metadata) {
                    console.log('Received metadata change:', data.metadata);
                    
                    // Update song metadata if present in the event
                    if (data.metadata.song) {
                        Object.assign(currentData.song, data.metadata.song);
                        updateSongInfo(currentData.song);
                        updateNowPlaying(currentData);
                    }
                }
                break;
            default:
                console.log('Unhandled event type:', eventType);}
    } else {
        console.log(`Event ${eventType} is for player ${playerName || 'unknown'}, but not relevant for current selection (${currentPlayerName || 'Default (Active Player)'}). Ignoring.`);
    }
}

/**
 * Get the name of the active player
 * @param {string} apiBase - The base URL for the API
 * @returns {Promise<string>} The name of the active player, or null if no player is active
 */
async function retrieve_active_player(apiBase = PLAYER_CONFIG.apiBasePath) {
    try {
        const response = await fetch(`${apiBase}/now-playing`);
        const data = await response.json();
        
        if (data && data.player && data.player.name) {
            console.log(`Retrieved active player name: ${data.player.name}`);
            return data.player.name;
        } else {
            console.warn('No active player found or player name not available');
            return null;
        }
    } catch (error) {
        console.error('Failed to get active player name:', error);
        return null;
    }
}

let progressInterval = null;
let lastProgressUpdate = null;

/**
 * Start auto progress updates for the progress bar
 * @param {Object} data - The current player data
 * @param {Element} progressBarEl - The progress bar element
 * @param {Element} positionEl - Element to display position text
 * @param {function} fetchCurrentPlayer - Function to fetch current player
 */
function startAutoProgress(data, progressBarEl, positionEl, fetchCurrentPlayer) {
    // Stop any existing interval first
    stopAutoProgress();
    
    // Only start if we have current data and a song is playing
    if (!data || !data.song || !data.song.duration) {
        console.log('Cannot start auto progress: missing song data or duration');
        return;
    }
    
    const isPlaying = data.state && data.state.toLowerCase() === 'playing';
    if (!isPlaying) {
        console.log('Cannot start auto progress: player is not in playing state');
        return;
    }
    
    console.log(`Starting auto progress updates from position: ${formatTime(data.position || 0)}`);
    
    // Ensure position is initialized
    if (data.position === undefined || data.position === null) {
        console.log('Initializing position to 0 as it was undefined');
        data.position = 0;
    }
    
    // Reset the timestamp to now
    lastProgressUpdate = Date.now();
    
    // Create a new interval
    progressInterval = setInterval(() => {
        if (!data || !data.song || !data.song.duration) {
            console.warn('Progress update: missing song data, stopping auto-updates');
            stopAutoProgress();
            return;
        }
        
        // Make sure position is defined
        if (data.position === undefined || data.position === null) {
            data.position = 0;
        }
        
        // Calculate how much time has passed since the last position update
        const now = Date.now();
        const elapsedSeconds = (now - lastProgressUpdate) / 1000;
        lastProgressUpdate = now;
        
        // Add the elapsed time to the current position
        data.position += elapsedSeconds;
        
        // Check if we've reached the end of the song
        if (data.position >= data.song.duration) {
            // Stop at the end of the song
            data.position = data.song.duration;
            
            // Force an update of player state from server when we reach the end
            console.log('Track reached the end, fetching current player state from server');
            fetchCurrentPlayer();
            
            // Don't stop the timer yet - it will be managed based on the updated state
        }
        
        // Update the progress bar and position display
        updateProgress(data.song, data.position, progressBarEl, positionEl);
    }, PLAYER_CONFIG.progressUpdateInterval);
}

/**
 * Stop auto progress updates
 */
function stopAutoProgress() {
    if (progressInterval) {
        clearInterval(progressInterval);
        progressInterval = null;
    }
}

// Export the new functions along with the existing ones
export {
    PLAYER_CONFIG,
    DEFAULT_CAPABILITIES,
    extractPlayerCapabilities,
    formatTime,
    fetchPlayers,
    fetchCurrentPlayer,
    fetchQueue,
    sendCommand,
    togglePlayPause,
    seekToPosition,
    cycleLoopMode,
    toggleShuffle,
    clearQueue,
    updateSongInfo,
    createPlayerWebSocket,
    updatePlayerInfo,
    updateNowPlaying,
    updateControlButtons,
    setupProgressBarClickHandler,
    updatePlayerDropdown,
    openPlayerLibrary,
    displayQueue,
    playQueueIndex,
    removeTrackFromQueue,
    toggleLike,    handlePlayerEvent,
    retrieve_active_player as change_active_player,
    updateProgress,
    startAutoProgress,
    stopAutoProgress
};

/**
 * Update progress bar and position text
 * @param {Object} song - The song object
 * @param {number} position - Current position in seconds
 * @param {Element} progressBarEl - The progress bar element
 * @param {Element} positionEl - Element to display position text
 */
function updateProgress(song, position, progressBarEl, positionEl) {
    if (song && song.duration && position !== undefined && position !== null) {
        const percentage = (position / song.duration) * 100;
        progressBarEl.style.width = `${percentage}%`;
        
        // Update position text if it exists in the DOM
        const posElement = positionEl.querySelector('div:last-child');
        if (posElement) {
            const formattedPosition = formatTime(position);
            const formattedDuration = formatTime(song.duration);
            posElement.innerHTML = `<strong>Position:</strong> ${formattedPosition} / ${formattedDuration}`;
        }
    } else {
        progressBarEl.style.width = '0%';
    }
}
