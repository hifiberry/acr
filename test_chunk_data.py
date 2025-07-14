#!/usr/bin/env python3
import socket
import struct
import time

# Create socket
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)

# Create test image data (small JPEG header)
jpeg_data = bytes([
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01,
    0x01, 0x01, 0x00, 0x48, 0x00, 0x48, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43
])

# Split into 3 chunks
chunk_size = 8
chunks = [jpeg_data[i:i+chunk_size] for i in range(0, len(jpeg_data), chunk_size)]
total_chunks = len(chunks)

print(f"Sending {total_chunks} chunks of test JPEG data")

# Send chunk 0 with actual image data (like packet #25)
chunk_0_data = chunks[0]
packet_0 = b"ssncchnk" + struct.pack(">II", 0, total_chunks) + b"ssncPICT" + chunk_0_data
sock.sendto(packet_0, ('127.0.0.1', 5555))
print(f"Sent chunk 0: {len(packet_0)} bytes")
time.sleep(0.1)

# Send chunk 1 header with padding (like packet #26)
packet_1 = b"ssncchnk" + struct.pack(">II", 1, total_chunks) + b"ssncPICT" + b"\x00" * (4096 - 24)
sock.sendto(packet_1, ('127.0.0.1', 5555))
print(f"Sent chunk 1: {len(packet_1)} bytes (header + padding)")
time.sleep(0.1)

# Send chunk 2 header with padding (like packet #27)
packet_2 = b"ssncchnk" + struct.pack(">II", 2, total_chunks) + b"ssncPICT" + b"\x00" * (4096 - 24)
sock.sendto(packet_2, ('127.0.0.1', 5555))
print(f"Sent chunk 2: {len(packet_2)} bytes (header + padding)")

sock.close()
print("Test data sent!")
