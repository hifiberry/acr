# Input sources

ACR can be controlled by USB HID remote controls (such as the HiFiBerry
USBRemote) and USB keyboards, via the `inputs` section of
`/etc/audiocontrol/audiocontrol.json`.

## Configuration

```json
"inputs": {
  "keyboard": {
    "enable": true,
    "volume_step": 5,
    "grab": false,
    "device": "",
    "keymap": { "KEY_ENTER": "playpause" }
  }
}
```

| Key | Default | Meaning |
|---|---|---|
| `enable` | `true` | Set `false` to disable keyboard input. |
| `volume_step` | `5` | Percentage points per volume action. |
| `grab` | `false` | Grab devices exclusively, so keys do not also reach the console. |
| `device` | `""` | Case-insensitive substring filter on the device name. Empty matches all devices. |
| `keymap` | built-in | **Replaces** the built-in map when present. |

Actions: `volume_up`, `volume_down`, `mute`, `play`, `pause`, `playpause`,
`stop`, `next`, `previous`.

Keys are `KEY_*` names (e.g. `KEY_VOLUMEUP`) or raw numeric codes (e.g. `190`)
for remotes emitting codes with no standard name.

## Default keymap

| Key | Action |
|---|---|
| `KEY_VOLUMEUP` / `KEY_VOLUMEDOWN` | `volume_up` / `volume_down` |
| `KEY_MUTE` | `mute` |
| `KEY_LEFT` / `KEY_UP` | `previous` |
| `KEY_RIGHT` / `KEY_DOWN` | `next` |
| `KEY_ENTER` | `playpause` |
| `KEY_PLAYPAUSE` | `playpause` |
| `KEY_PLAY` / `KEY_PAUSE` | `play` / `pause` |
| `KEY_NEXTSONG` / `KEY_PREVIOUSSONG` | `next` / `previous` |
| `KEY_STOPCD` | `stop` |

Because `KEY_ENTER` and the arrow keys are mapped, an attached USB keyboard also
acts as a remote. Use `device` to restrict input to a specific device.

On a Raspberry Pi with an HDMI display connected, the kernel's HDMI-CEC virtual
keyboards (`vc4-hdmi-0`, `vc4-hdmi-1`) also match the default keymap (10 of the
14 keys above), so a TV remote can drive playback over CEC as well. This is
expected, and generally desirable; use `device` if you need to exclude it.

Holding a volume key ramps the volume. Holding a transport key acts once.

## Diagnostics

```
$ audiocontrol_input_devices
/dev/input/event0    "HiFiBerry USBRemote"        MATCHED (8 mapped keys)
/dev/input/event1    "Power Button"               no mapped keys

$ audiocontrol_input_devices --watch
press a key... (Ctrl-C to stop)
  KEY_VOLUMEUP (115)  -> volume_up
  KEY_HOMEPAGE (172)  -> unmapped
```

`audiocontrol_input_devices` also takes `--config`/`-c` to point at a
non-default config file (default `/etc/audiocontrol/audiocontrol.json`).

`GET /api/inputs` reports the same information as JSON.

## Troubleshooting

**The remote does nothing.**

1. `audiocontrol_input_devices` — is the device listed and MATCHED?
2. Not listed at all, or a permission error? Check the `input` group:
   `id audiocontrol` should include it. If not:
   `sudo usermod -a -G input audiocontrol && sudo systemctl restart audiocontrol`
3. Listed but not MATCHED? Run `--watch` and press a button. Add the reported
   code to `keymap`.
4. Plugged the remote in after boot? Device discovery runs at startup only:
   `sudo systemctl restart audiocontrol`.
