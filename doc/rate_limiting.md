# Rate Limiting in AudioControl

AudioControl implements a comprehensive rate limiting system to ensure that API calls to external services are properly throttled. This prevents being blocked by services for making too many requests too quickly and ensures respectful usage of third-party APIs.

## Overview

The rate limiting system is centralized and service-agnostic, allowing each service to register its own rate limits and apply them consistently across all API calls.

## Architecture

### Core Components

1. **Rate Limiter Module** (`src/helpers/ratelimit.rs`)
   - Central rate limiting implementation
   - Per-service rate limit registration and enforcement
   - Thread-safe implementation using mutexes

2. **Service Registration**
   - Each service registers its rate limit during initialization
   - Default values are provided but can be overridden via configuration

3. **Rate Limit Application**
   - Services call `ratelimit::rate_limit("service_name")` before making API requests
   - The rate limiter ensures minimum time intervals between requests

## Supported Services

### Last.fm

**Default Rate Limit:** 1000ms (1 second) between requests

```rust
// Registration during initialization
ratelimit::register_service("lastfm", 1000);

// Application before API calls
ratelimit::rate_limit("lastfm");
```

**Configuration:**
```json
{
  "services": {
    "lastfm": {
      "enable": true,
      "rate_limit_ms": 1000
    }
  }
}
```

### TheAudioDB

**Default Rate Limit:** 500ms (0.5 seconds) between requests

```rust
// Registration during initialization
ratelimit::register_service("theaudiodb", 500);

// Application before API calls
ratelimit::rate_limit("theaudiodb");
```

**Configuration:**
```json
{
  "services": {
    "theaudiodb": {
      "enable": true,
      "api_key": "your_api_key_here",
      "rate_limit_ms": 500
    }
  }
}
    ```

### MusicBrainz

**Default Rate Limit:** 1000ms (1 second) between requests

```rust
// Registration during initialization
ratelimit::register_service("musicbrainz", 1000);

// Application before API calls
ratelimit::rate_limit("musicbrainz");
```

**Configuration:**
```json
{
  "services": {
    "musicbrainz": {
      "enable": true,
      "rate_limit_ms": 1000
    }
  }
}
```

### FanArt.tv

**Default Rate Limit:** 500ms (0.5 seconds) between requests

```rust
// Registration during initialization
ratelimit::register_service("fanarttv", 500);

// Application before API calls
ratelimit::rate_limit("fanarttv");
```

**Configuration:**
```json
{
  "services": {
    "fanarttv": {
      "enable": true,
      "api_key": "your_api_key_here",
      "rate_limit_ms": 500
    }
  }
}
```

## Implementation Details

### Service Integration Pattern

Each service follows a consistent pattern for rate limiting integration:

1. **Initialize Rate Limiting**
   ```rust
   pub fn initialize_from_config(config: &serde_json::Value) {
       // ... other initialization code ...
       
       let rate_limit_ms = config.get("rate_limit_ms")
           .and_then(|v| v.as_u64())
           .unwrap_or(DEFAULT_RATE_LIMIT_MS);
           
       ratelimit::register_service("service_name", rate_limit_ms);
   }
   ```

2. **Apply Rate Limiting**
   ```rust
   pub fn api_call(&self) -> Result<Response, Error> {
       // Apply rate limiting before making the request
       ratelimit::rate_limit("service_name");
       
       // Make the actual API request
       // ...
   }
   ```

### Thread Safety

The rate limiting system is fully thread-safe and can handle concurrent requests from multiple threads. Each service maintains its own rate limit state independently.

### Error Handling

Rate limiting failures are handled gracefully:
- If rate limit registration fails, the service continues with best-effort behavior
- Rate limit enforcement is non-blocking and doesn't throw errors
- Services can function normally even if rate limiting is disabled

## Configuration

### Global Rate Limiting Settings

Rate limits can be configured globally or per-service in the main configuration file:

```json
{
  "services": {
    "rate_limiting": {
      "enable": true,
      "default_rate_limit_ms": 1000
    },
    "lastfm": {
      "enable": true,
      "rate_limit_ms": 1500
    },
    "theaudiodb": {
      "enable": true,
      "rate_limit_ms": 750
    }
  }
}
```

### Service-Specific Overrides

Each service can override the default rate limit:

- **`rate_limit_ms`**: Minimum milliseconds between requests
- **`enable`**: Whether the service is enabled (affects rate limiting registration)

### Recommended Settings

| Service | Recommended Rate Limit | Reasoning |
|---------|----------------------|-----------|
| Last.fm | 1000ms (1 req/sec) | Last.fm has generous rate limits but recommends no more than 5 req/sec |
| TheAudioDB | 500ms (2 req/sec) | TheAudioDB allows up to 2 requests per second for API users |
| MusicBrainz | 1000ms (1 req/sec) | MusicBrainz rate limit is 1 req/sec, strictly enforced |
| FanArt.tv | 500ms (2 req/sec) | FanArt.tv allows 2 requests per second for personal API keys |

## Monitoring and Debugging

### Logging

Rate limiting activities are logged at appropriate levels:

```rust
info!("Service rate limit set to {} ms", rate_limit_ms);
debug!("Rate limiting applied for service: {}", service_name);
```

### Metrics

The rate limiting system can be extended to provide metrics:
- Total requests per service
- Average wait times
- Rate limit violations (if any)

## Best Practices

### For Developers

1. **Always Register Rate Limits**
   ```rust
   // Register during service initialization
   ratelimit::register_service("myservice", 1000);
   ```

2. **Apply Rate Limiting Before API Calls**
   ```rust
   // Apply before every external API request
   ratelimit::rate_limit("myservice");
   let response = make_api_request();
   ```

3. **Use Appropriate Default Values**
   ```rust
   // Conservative defaults that respect service limits
   let rate_limit_ms = config.get("rate_limit_ms")
       .and_then(|v| v.as_u64())
       .unwrap_or(1000); // 1 second default
   ```

4. **Handle Configuration Gracefully**
   ```rust
   // Continue with defaults if configuration is missing
   if let Some(service_config) = get_service_config(config, "myservice") {
       // Use configured values
   } else {
       // Fall back to sensible defaults
       ratelimit::register_service("myservice", 1000);
   }
   ```

### For Administrators

1. **Monitor API Usage**
   - Check service logs for rate limiting messages
   - Adjust rate limits based on service requirements
   - Consider API quotas and usage patterns

2. **Configure Appropriately**
   - Start with conservative settings
   - Increase rate limits only if needed and within service limits
   - Test configuration changes in non-production environments

3. **Service-Specific Considerations**
   - **Last.fm**: Has daily and monthly quotas in addition to rate limits
   - **TheAudioDB**: Requires API key for higher rate limits
   - **MusicBrainz**: Strictly enforces 1 req/sec limit
   - **FanArt.tv**: Personal vs. commercial API keys have different limits

## Troubleshooting

### Common Issues

1. **"Service not registered" errors**
   - Ensure `ratelimit::register_service()` is called during initialization
   - Check that the service name matches between registration and usage

2. **API requests being blocked by services**
   - Decrease rate limit values in configuration
   - Check service-specific rate limit documentation
   - Verify API key validity and quotas

3. **Performance issues**
   - Rate limiting adds small delays between requests
   - Consider if aggressive rate limiting is necessary for your use case
   - Balance between performance and service compliance

### Debug Configuration

For debugging rate limiting issues:

```json
{
  "services": {
    "rate_limiting": {
      "enable": true,
      "debug": true
    },
    "lastfm": {
      "rate_limit_ms": 2000
    }
  },
  "logging": {
    "level": "debug"
  }
}
```

## Future Enhancements

Potential improvements to the rate limiting system:

1. **Adaptive Rate Limiting**
   - Automatically adjust based on service responses
   - Implement exponential backoff for failed requests

2. **Burst Handling**
   - Allow short bursts within overall rate limits
   - Token bucket algorithm implementation

3. **Service Health Monitoring**
   - Track service availability and response times
   - Temporarily disable services that are down

4. **Configuration Validation**
   - Validate rate limit values against known service limits
   - Warn about potentially problematic configurations

## Conclusion

The rate limiting system in AudioControl ensures respectful usage of external APIs while maintaining good performance. By following the established patterns and configuration guidelines, services can be integrated reliably while avoiding rate limit violations.

For questions or issues related to rate limiting, check the service logs and ensure proper configuration according to this documentation.
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
