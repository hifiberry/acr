# Rate Limiting in ACR

ACR interacts with multiple third-party APIs that may impose rate limits. To ensure good behavior and avoid being blocked, ACR implements a rate limiting mechanism that controls the frequency of API requests.

## RateLimiter Module

The `ratelimit.rs` module provides a singleton rate limiter that can enforce different rate limits for different services. The rate limiter ensures that consecutive API calls to the same service respect a minimum time delay between requests.

### Features

- **Per-Service Rate Limits**: Different services can have different rate limits
- **Default Rate Limit**: Unregistered services default to 500ms (2 requests per second)
- **Configurable**: Rate limits can be configured via the application configuration
- **Thread-Safe**: Implemented using a mutex for thread safety
- **Automatic Throttling**: Automatically sleeps when necessary to enforce rate limits

## Usage

### Registering a Service

Before making API calls, register the service with a rate limit:

```rust
use crate::helpers::ratelimit;

// Register a rate limit of 500ms (2 requests per second) for "my-service"
ratelimit::register_service("my-service", 500);
```

### Applying Rate Limiting

Before making an API call, apply rate limiting:

```rust
// This will sleep if necessary to respect the rate limit
ratelimit::rate_limit("my-service");

// Now make your API call
let response = client.get("https://api.example.com/resource");
```

### Configuration

Rate limits can be configured in the `acr.json` configuration file:

```json
{
  "theartistdb": {
    "enable": true,
    "api_key": "YOUR_API_KEY_HERE",
    "rate_limit_ms": 500
  },
  "musicbrainz": {
    "enable": true,
    "rate_limit_ms": 1000
  }
}
```

## Example: TheArtistDB Implementation

The TheArtistDB module demonstrates how to use the rate limiter:

1. **Register the service during initialization**:
   ```rust
   // In initialize_from_config()
   let rate_limit_ms = artistdb_config.get("rate_limit_ms")
       .and_then(|v| v.as_u64())
       .unwrap_or(500);
       
   ratelimit::register_service("theartistdb", rate_limit_ms);
   ```

2. **Apply rate limiting before making API calls**:
   ```rust
   // In lookup_artistdb_by_mbid()
   ratelimit::rate_limit("theartistdb");
   
   // Now make the API request
   let response_text = client.get_text(&url);
   ```

## Best Practices

1. **Choose Appropriate Rate Limits**: Most APIs document their rate limits; respect them
2. **Register Early**: Register services during initialization to ensure rate limits are applied from the start
3. **Apply Before Each Request**: Apply rate limiting immediately before making any API request
4. **Make Rate Limits Configurable**: Allow users to adjust rate limits through configuration

## Implementation Details

The `RateLimiter` maintains a map of service names to their last access time and minimum delay. When `rate_limit()` is called, it:

1. Retrieves the last access time and minimum delay for the service
2. Calculates how much time has elapsed since the last access
3. If less than the minimum delay has passed, sleeps for the remaining time
4. Updates the last access time to the current time

This ensures that consecutive calls to the same service are spaced at least by the minimum delay.
