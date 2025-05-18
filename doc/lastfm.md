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

1. **Initiate Connection from Frontend**: Your application's frontend should provide a user interface element (e.g., a "Connect to Last.fm" button).
2. **Trigger Authentication Flow**: Clicking this element should trigger a call from your frontend to the `GET /api/lastfm/auth` backend endpoint.
   - The backend's `get_auth_url_handler` requests a temporary request token from Last.fm and constructs a Last.fm authorization URL. This URL (e.g., `http://www.last.fm/api/auth/?api_key=YOUR_API_KEY&token=TEMP_REQUEST_TOKEN`) and the temporary request token are returned to your frontend.
3. **User Authorization at Last.fm**: Your frontend redirects the user's browser to the authorization URL provided by the backend.
   - The user will be prompted to log in to Last.fm (if not already logged in) and then asked to authorize your application (which should be registered with Last.fm to obtain an API key).
4. **Handle Callback and Token Preparation**: After the user grants authorization, Last.fm redirects their browser back to a callback URL that you configured when you registered your application with Last.fm. Your frontend application at this callback URL must:
   - Extract any necessary parameters from the callback (Last.fm typically appends the `token` to the callback URL, but this is the *same* request token from step 2, used for verification by Last.fm, not the one ACR needs for `prepare_complete_auth`).
   - Take the *original temporary request token* (obtained in step 2 from `/api/lastfm/auth`) and send it to the backend via a `POST` request to `/api/lastfm/prepare_complete_auth`.
   - The backend's `prepare_complete_auth` handler stores this request token, marking it as ready to be exchanged for a session key.
5. **Finalizing Authentication**: Your frontend then makes a `GET` request to `/api/lastfm/complete_auth`.
   - The backend's `complete_auth` handler uses the stored temporary request token to call Last.fm's `auth.getSession` method.
   - If successful, Last.fm returns a permanent `session_key` and your `username`. These are stored securely by ACR.
   - The frontend is notified of the successful authentication and updates the UI to show you as connected.

## Disconnection

To disconnect from Last.fm:

1. **Initiate Disconnection from Frontend**: Your application's frontend should provide a user interface element (e.g., a "Disconnect from Last.fm" button).
2. **Trigger Disconnection Flow**: Clicking this element should trigger a call from your frontend to the `POST /api/lastfm/disconnect` backend endpoint.
   - The backend's `disconnect_handler` clears the stored `session_key` and `username` both from memory and the secure store.
   - The frontend is notified and updates the UI to show you as disconnected.

## API Endpoints

The following API endpoints are available for Last.fm integration:

- `GET /api/lastfm/status`: Retrieves the current authentication status (whether a user is connected and their username if so).
- `GET /api/lastfm/auth`: Initiates the authentication process by providing a Last.fm authorization URL and a temporary request token. The frontend uses this URL to redirect the user to Last.fm for authorization.
- `POST /api/lastfm/prepare_complete_auth`: Allows the frontend to send the temporary request token (obtained from `/auth`) back to the backend after the user has authorized the application on Last.fm. This prepares the backend to exchange the request token for a session key.
  - **Request Body**: `{"token": "TEMP_REQUEST_TOKEN"}`
- `GET /api/lastfm/complete_auth`: Finalizes the authentication process. The backend uses the previously prepared request token to obtain a permanent session key from Last.fm.
- `POST /api/lastfm/disconnect`: Disconnects the currently authenticated user by clearing their session details.
- `GET /api/lastfm/loved_tracks`: Retrieves a list of the authenticated user's "loved" tracks from Last.fm.

## Frontend Integration Guide

To integrate Last.fm functionality into a custom web interface or application, developers should:

1. **Implement UI Elements**: Create buttons or links for:
   - Connecting to Last.fm.
   - Disconnecting from Last.fm.
   - Displaying authentication status (e.g., "Connected as [username]").
2. **Handle Authentication Flow**:
   - On "Connect":
     - Call `GET /api/lastfm/auth` to get the Last.fm authorization URL and the `request_token`.
     - Store the `request_token` securely in the frontend (e.g., session storage or a state management solution).
     - Redirect the user to the Last.fm authorization URL.
   - On the designated callback page (after user authorizes on Last.fm):
     - Retrieve the stored `request_token`.
     - Call `POST /api/lastfm/prepare_complete_auth` with the `request_token`.
     - If successful, call `GET /api/lastfm/complete_auth` to finalize authentication.
     - Update UI to reflect connected status.
3. **Handle Disconnection Flow**:
   - On "Disconnect":
     - Call `POST /api/lastfm/disconnect`.
     - Update UI to reflect disconnected status.
4. **Display Status**:
   - Periodically, or on page load, call `GET /api/lastfm/status` to keep the UI synchronized with the actual authentication state.
5. **Utilize Other Endpoints**:
   - Implement features to call `GET /api/lastfm/loved_tracks` and display the results as needed.

The ACR backend provides the necessary API endpoints; the frontend is responsible for orchestrating the user interaction and API calls as described.
