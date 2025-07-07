# Systemd Unit Integration

The librespot and RAAT player controllers now support systemd unit checking to ensure the required services are running before attempting to initialize the player.

## Configuration

### Librespot Player

The librespot player now supports a `systemd_unit` configuration option:

```json
{
  "librespot": {
    "enable": true,
    "process_name": "/usr/bin/librespot",
    "systemd_unit": "librespot"
  }
}
```

- `systemd_unit`: The name of the systemd unit to check (default: "librespot")
- If set to an empty string, systemd checking is disabled
- If the unit is not active, a warning is logged but the player still initializes

### RAAT Player

The RAAT player now supports a `systemd_unit` configuration option:

```json
{
  "raat": {
    "enable": true,
    "metadata_pipe": "/var/run/raat/metadata_pipe",
    "control_pipe": "/var/run/raat/control_pipe",
    "reopen_metadata_pipe": true,
    "systemd_unit": "raat"
  }
}
```

- `systemd_unit`: The name of the systemd unit to check (default: "raat")
- If set to an empty string, systemd checking is disabled
- If the unit is not active, a warning is logged but the player still initializes

## Behavior

When a player is initialized with systemd unit checking enabled:

1. The player controller will check if the specified systemd unit is active
2. If the unit is active, a debug message is logged
3. If the unit is not active, a warning is logged but the player continues to initialize
4. If there's an error checking the unit (e.g., systemd not available), a warning is logged but the player continues to initialize

This approach ensures that the player can still work even if systemd is not available or if there are permission issues, while providing useful feedback when the expected service is not running.

## Examples

### Working Service

```
DEBUG: Systemd unit 'librespot' is active
```

### Inactive Service

```
WARN: Systemd unit 'librespot' is not active - librespot player may not work correctly
```

### Error Checking Service

```
WARN: Could not check systemd unit 'librespot': systemd is not available on this system - continuing anyway
```

## Disabling Systemd Checking

To disable systemd checking, set the `systemd_unit` option to an empty string:

```json
{
  "librespot": {
    "enable": true,
    "process_name": "/usr/bin/librespot",
    "systemd_unit": ""
  }
}
```

## Legacy Compatibility

Players configured without the `systemd_unit` option will use the default unit names:

- librespot: "librespot"
- raat: "raat"

To maintain backward compatibility, existing configurations will continue to work without modification.
