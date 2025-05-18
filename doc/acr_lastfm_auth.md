# acr_lastfm_auth

The `acr_lastfm_auth` tool provides a command-line interface for authenticating with Last.fm using their desktop authentication flow. This tool helps set up Last.fm integration, enabling features like scrobbling, "now playing" updates, and accessing user profile data.

## Overview

Last.fm requires authentication before any user-specific actions (like scrobbling) can be performed. This tool implements the desktop authentication flow as described in [Last.fm's API documentation](https://www.last.fm/api/desktopauth).

## Usage

### Initial Authentication

To authenticate with Last.fm for the first time, you need a Last.fm API key and API secret. You can obtain these by creating an API account at [Last.fm's API page](https://www.last.fm/api/account/create).

```bash
acr_lastfm_auth --api-key YOUR_API_KEY --api-secret YOUR_API_SECRET
```

This will:
1. Generate an authentication URL
2. Ask you to visit the URL in your browser
3. After you authorize the application on the Last.fm website, you can return to the terminal and press Enter
4. The tool will obtain a session key and save your credentials for future use

### Using Saved Credentials

Once you've completed the initial authentication, you can use saved credentials in future sessions:

```bash
acr_lastfm_auth --use-saved
```

By default, credentials are saved to `lastfm_credentials.json` in the current directory. You can specify a different location with the `--credentials-file` option.

### Full Options

```
USAGE:
    acr_lastfm_auth [OPTIONS]

OPTIONS:
    --api-key <KEY>               API Key (required for initial authentication)
    --api-secret <SECRET>         API Secret (required for initial authentication)
    --credentials-file <PATH>     Path to save or load credentials file [default: lastfm_credentials.json]
    --use-saved                   Authenticate with saved credentials
    -h, --help                    Print help information
    -V, --version                 Print version information
```

## Example Session

Here's an example of a complete authentication session:

```
$ acr_lastfm_auth --api-key d45f86c40a6f3684acaad9c212872f77 --api-secret 7886378c6142e9f9e44522b0f61fc691

To authenticate with Last.fm, please:
1. Visit this URL in your browser: https://www.last.fm/api/auth/?api_key=d45f86c40a6f3684acaad9c212872f77&token=8c76d493ac57194ab4280c5ac79cf0c5
2. Log in to your Last.fm account if necessary
3. Authorize this application
4. Return here and press Enter when completed

Authentication successful!
Username: ExampleUser
Session key: KJHBGFDSewrt5678ihgfKJHGF

Credentials saved to: lastfm_credentials.json
You can use these credentials in the future with --use-saved
```

## Integration with ACR

Once authenticated, the Last.fm integration in ACR will be able to:
- Scrobble tracks to your Last.fm profile
- Update "Now Playing" status
- Access user profile data and recommendations

The credentials stored by this tool will be used automatically by the ACR Last.fm module when needed.

## Troubleshooting

- **Authentication fails**: Make sure you've authorized the application on the Last.fm website before pressing Enter in the terminal.
- **API errors**: Check that your API key and secret are correct.
- **File permission errors**: Ensure you have write permissions to the directory where the credentials file will be saved.

## Security Considerations

The credentials file contains your Last.fm API key, API secret, and session key. Keep this file secure as it grants access to your Last.fm account. The session key does not expire, so anyone with access to this file could potentially access your Last.fm account.
