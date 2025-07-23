#!/bin/bash

# Script to generate test album with 20 sine wave tracks
# Each track has a different frequency and embedded cover art

ALBUM_DIR="test_album_sine_waves"
ARTIST="Sine wave generator"
ALBUM="Frequency Test Album"
YEAR="2025"
GENRE="Test tone"

cd "$(dirname "$0")"

echo "Generating 20 sine wave tracks with embedded cover art..."

for i in {1..20}; do
    FREQ=$((i * 100))
    TRACK_NAME="${FREQ}Hz"
    FILENAME=$(printf "%02d_${FREQ}Hz.mp3" $i)
    
    echo "Creating track $i: $TRACK_NAME ($FREQ Hz)"
    
    # Create a cover image with the frequency displayed
    convert -size 300x300 xc:lightblue \
        -fill darkblue -pointsize 36 -gravity center \
        -annotate +0-50 "$TRACK_NAME" \
        -annotate +0+0 "$ARTIST" \
        -annotate +0+50 "$ALBUM" \
        -quality 85 "/tmp/cover_${FREQ}.jpg"
    
    # Generate the audio file
    ffmpeg -f lavfi -i "sine=frequency=${FREQ}:duration=10" \
        -c:a libmp3lame -b:a 64k -ar 44100 -ac 2 \
        -metadata artist="$ARTIST" \
        -metadata album="$ALBUM" \
        -metadata title="$TRACK_NAME" \
        -metadata track="$i/20" \
        -metadata date="$YEAR" \
        -metadata genre="$GENRE" \
        -y "/tmp/temp_${FREQ}.mp3" > /dev/null 2>&1
    
    # Embed the cover art into the MP3
    ffmpeg -i "/tmp/temp_${FREQ}.mp3" -i "/tmp/cover_${FREQ}.jpg" \
        -map 0:0 -map 1:0 -c copy -id3v2_version 3 \
        -metadata:s:v title="Album cover" \
        -metadata:s:v comment="Cover (front)" \
        -y "$ALBUM_DIR/$FILENAME" > /dev/null 2>&1
    
    # Clean up temporary files
    rm "/tmp/cover_${FREQ}.jpg" "/tmp/temp_${FREQ}.mp3"
done

echo "Generated 20 tracks in $ALBUM_DIR/"
echo "Each track has embedded cover art showing its frequency"
