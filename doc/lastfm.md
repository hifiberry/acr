# Last.fm Integration

The ACR (AudioControl3) application now includes integration with Last.fm, allowing scrobbling and "now playing" updates for your music listening. This document explains how to set up and use the Last.fm integration.

## Features

- Secure storage of Last.fm credentials using AES-GCM encryption
- Authentication with Last.fm via OAuth
- Scrobbling of tracks as you listen
- "Now playing" status updates
- Web interface to manage your Last.fm connection

## Configuration

The Last.fm integration is controlled via the `acr.json` configuration file:

```json
{
  "lastfm": {
    "enable": true
  }
}
```

## Security

Last.fm credentials are securely stored using AES-GCM encryption in the security store. The path to the security store can be configured in the `acr.json` file:

```json
{
  "general": {
    "security_store": "secrets/security_store.json"
  }
}
```

## Authentication

To authenticate with Last.fm:

1. Navigate to the Last.fm web interface at `http://your-device-ip:1080/example/lastfm.html`
2. Click the "Connect to Last.fm" button
3. You will be redirected to Last.fm to authorize the application
4. After authorization, you will be redirected back and automatically logged in

## API Endpoints

The following API endpoints are available for Last.fm integration:

- `GET /api/lastfm/status` - Get the current Last.fm authentication status
- `GET /api/lastfm/auth` - Get a Last.fm authentication URL
- `GET /api/lastfm/callback?token=<token>` - Complete Last.fm authentication with a token

## Web Interface

A web interface is provided for managing your Last.fm connection at:

```
http://your-device-ip:1080/example/lastfm.html
```

## Troubleshooting

If you encounter issues with the Last.fm integration:

1. Check if Last.fm is enabled in the configuration
2. Verify that the security store is properly initialized
3. Check if the API keys are properly set (they must be set at build time in the secrets.txt file)
4. Check the application logs for error messages

## Developer Information

The Last.fm integration consists of:

- `helpers/lastfm.rs` - Core Last.fm client implementation
- `api/lastfm.rs` - API endpoints for Last.fm
- `security_store.rs` - Secure storage for Last.fm credentials
- `example-app/lastfm.html` - Web interface for Last.fm
