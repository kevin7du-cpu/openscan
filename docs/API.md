# OpenScan process API, protocol version 1

Run `OpenScan.exe COMMAND`. All non-GUI requests are JSON on stdin; close stdin
after writing. One JSON response is written to stdout. Drain stderr separately
(OCR may report diagnostics there). No listening port, server or shell is needed.
Use your language's argument-list subprocess API, not shell string concatenation.

Every response contains `protocolVersion: 1` and `ok`. Error responses include
`error`; exit code 1 means failure. A partial scan returns exit code 2 with
`result.warning`, a recoverable session directory and any saved partial output.
GUI cancellation is a successful reply with `cancelled: true, path: null`.

| Command | Request | Response data |
|---|---|---|
| `version` | None | Module version, drivers and OCR availability |
| `devices` | None | `devices: [{id,name,driver}]` |
| `capabilities` | `{ "deviceId": "…" }` | `capabilities.sources` with source/resolutions |
| `dialog --return-json` | None | Scan/review window, then `{path,cancelled}` |
| `presets-dialog` | None | Preset manager; returns after closing |
| `scan` | `{profile,output,sessionDirectory?}` | Saved PDF, session and acquisition result |
| `process` | `{profile,images:[paths],output}` | Correct imported images, save PDF, return session/pages |
| `presets` | None | Presets from the default local store |
| `preset-list` | `{directory}` | Presets from a specified store |
| `preset-save` | `{profile,overwrite?:false,directory?}` | Save success |
| `preset-delete` | `{name,directory?}` | Delete success |

`dialog --session DIRECTORY` reopens a retained session. `process` accepts single
frame images; the GUI import also expands multi-page TIFF files. PNG/JPEG export
is exposed in `PdfWriter.ExportImages`; the GUI's image export uses PNG.

Example profile (all fields other than a device for physical acquisition have defaults):

```json
{
  "schemaVersion": 1,
  "name": "Office documents",
  "deviceId": "ID returned by devices",
  "source": "duplex",
  "color": "grayscale",
  "paper": "letter",
  "dpi": 300,
  "maxPages": 100,
  "nativeInterface": false,
  "flipDuplexBacks": false,
  "searchablePdf": true,
  "jpegQuality": 90,
  "cleanup": {
    "deskew": true,
    "autoCrop": false,
    "removeBackground": false,
    "denoise": true,
    "sharpen": false,
    "skipBlankPages": true,
    "blankInkPercent": 0.08,
    "brightness": 0,
    "contrast": 0,
    "rotateDegrees": 0
  }
}
```

Sources: `flatbed`, `feeder`, `duplex`. Colors: `color`, `grayscale`, `blackWhite`.
Paper: `letter`, `a4`, `legal`, `a5`. Brightness/contrast: −100…100. Rotation:
0/90/180/270. Unknown JSON fields are rejected to catch integration mistakes.
Headless `scan` does not allow `nativeInterface: true`.

Do not assume `ok:true` merely because a process exited. Check the protocol and
response. Retain the full bundle, and verify the returned file exists before opening
it. The PdfCraft adapter bounds responses to 4 MB and headless calls to 15 minutes.
The UI has no arbitrary timeout. Run one physical acquisition at a time per device.

This is a local trusted-host API, not a network service or a sandbox. Hosts must
enforce their own allowed filesystem roots before submitting file paths.
