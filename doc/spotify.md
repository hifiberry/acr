# Spotify Integration for Audiocontrol

This document describes how to use the Spotify integration for AudioControl Rust (Audiocontrol).

## Overview

The Spotify integration allows users to authenticate with Spotify and use their Spotify accounts with the Audiocontrol system. It leverages OAuth2 authentication using the [hifiberry/oauth-spotify](https://github.com/hifiberry/oauth-spotify) proxy to handle authorization with Spotify.

## Setup

### Prerequisites

1. A Spotify account
2. A Spotify Developer account with an application registered
3. An instance of the `oauth-spotify` proxy running on a publicly accessible server

### Configuration

1. In your Spotify Developer Dashboard, add the OAuth redirect URI that points to your `oauth-spotify` proxy's callback URL
2. Configure your `spotify.html` example application with:
   - Your Spotify Client ID
   - The URL of your OAuth proxy server
   - The proper redirect URI

## Authentication Flow

The authentication flow follows these steps:

1. User clicks "Connect to Spotify" in the web interface
2. The application creates a session with the OAuth proxy
3. User is redirected to Spotify's authorization page
4. After authorizing, Spotify redirects to the OAuth proxy
5. The proxy handles the token exchange with Spotify
6. The client polls the proxy for the authentication result
7. When tokens are received, they're stored securely in the Audiocontrol security store

## API Endpoints

The following API endpoints are available:

### `POST /api/spotify/tokens`

Stores Spotify access and refresh tokens.

**Request body:**
```json
{
  "access_token": "string",
  "refresh_token": "string",
  "expires_in": number
}
```

**Response:**
```json
{
  "status": "success",
  "message": "Tokens stored successfully",
  "expires_at": number
}
```

### `GET /api/spotify/status`

Gets the current Spotify authentication status.

**Response:**
```json
{
  "authenticated": boolean,
  "expires_at": number | null
}
```

### `POST /api/spotify/logout`

Clears all Spotify tokens and logs the user out.

**Response:**
```json
{
  "status": "success",
  "message": "Logged out successfully"
}
```

## Security

The Spotify tokens are stored in the Audiocontrol security store, which encrypts sensitive data using AES-256-GCM encryption. The encryption key is defined in the `secrets.txt` file.

## Example Implementation

An example implementation is provided in the `example/web/spotify.html` file. This web page demonstrates how to:

1. Check authentication status
2. Connect to Spotify using the OAuth flow
3. Store and manage tokens
4. Display connection status

To use this example, you must update the following variables in the JavaScript:

- `OAUTH_PROXY_URL`: The URL of your OAuth proxy server
- `CLIENT_ID`: Your Spotify application client ID

## Adding Spotify Support to Other Applications

To add Spotify support to your own Audiocontrol-based applications, you'll need to:

1. Import the `crate::helpers::spotify::Spotify` module
2. Use the module's methods to check authentication, retrieve tokens, etc.
3. Implement the OAuth flow as shown in the example

## Limitations

- The tokens have a limited lifetime (typically 1 hour)
- Refresh tokens are not automatically used to get new tokens
- This integration only handles authentication, not actual Spotify API usage
