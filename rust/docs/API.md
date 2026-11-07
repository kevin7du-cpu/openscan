# JSON protocol v1

Spawn the existing platform binary with the command below, write one UTF-8 JSON
request to stdin, close stdin, then read stdout and wait for exit. Windows marks
the binary as a GUI app; use an actual child process with redirected streams rather
than relying on an interactive shell's treatment of GUI processes.

Every reply has `protocolVersion: 1` and `ok`. Failure exits nonzero and has `error`
or a partial scan's `result.warning`. Output is bounded to 4 MB by the PDFCraft host.

| Command | Request / arguments |
| --- | --- |
| `version` | No input; reports implementation, version, backends and OCR availability |
| `devices` | No input; installed OS drivers only; `--network` explicitly adds mDNS |
| `capabilities` | `{"deviceId":"…"}` |
| `scan` | `{"profile":{…},"output":"absolute new PDF path","sessionDirectory":"absolute new directory"}` |
| `process` | `{"images":["absolute image path"],"profile":{…},"output":"absolute new PDF path"}` |
| `preset-list` | `{"directory":"absolute preset directory"}` |
| `preset-save` | `{"directory":"…","profile":{…},"overwrite":false}` |
| `preset-delete` | `{"directory":"…","name":"profile name"}` |
| `dialog` | `--return-json` closes after Save PDF and returns `path`; `--session <directory>` restores pages |
| `presets-dialog` | Opens the preset manager; no acquisition starts automatically |

Directories are optional for standalone use; hosts should supply explicit paths.
Dialog cancellation returns `ok:true,cancelled:true,path:null`.

Profiles accept `schemaVersion` (1), `name`, `deviceId`, `source`
(`flatbed`, `feeder`, `duplex`), `color` (`color`, `grayscale`, `blackWhite`), `paper`
(`letter`, `a4`, `legal`, `a5`), `dpi` (75–600), `maxPages` (1–250),
`flipDuplexBacks`, `searchablePdf`, `jpegQuality` (20–100), and `cleanup`.
`nativeInterface` remains reserved for protocol compatibility; this implementation
uses its own settings and does not expose a vendor dialog.

Cleanup fields: `deskew`, `autoCrop`, `removeBackground`, `denoise`, `sharpen`,
`skipBlankPages`, `blankInkPercent` (0–5), `brightness` and `contrast` (-100–100),
`rotateDegrees` (0/90/180/270). Defaults permit a small request such as
`{"name":"Letters","deviceId":"…","source":"feeder","dpi":300}`.

Unknown fields are rejected. Failed acquisition preserves already recorded originals
in the session. Export never silently overwrites an existing file. Hosts should expose
partial warnings and recovery locations to their user.

Environment overrides must be absolute paths: `OPENSCAN_MODELS` for verified OCR
models and `OPENSCAN_SCANIMAGE` for an installed SANE driver executable.
