# SpeakEasy Architecture

## Project Overview

SpeakEasy is a completely local, free, real-time speech-to-text tool. It records audio from your microphone and transcribes it using OpenAI's Whisper model (via whisper.cpp) running locally as a native macOS app. Transcriptions are optionally refined by a local LLM via Ollama for polished output. No cloud APIs, no API keys, no cost.

## Current Architecture — Native Swift App

The app is built from Swift sources in `Sources/` and compiled against a local whisper.cpp build. A legacy Python prototype exists (`main.py`, `refiner.py`) but the primary app is the native `.app` bundle.

### Files

| File | Purpose |
|------|---------|
| `Sources/main.swift` | Entry point — creates NSApplication and AppDelegate |
| `Sources/AppDelegate.swift` | Window, menu bar icon, settings UI, pipeline orchestration |
| `Sources/KeyMonitor.swift` | Listens for Option key via NSEvent global/local monitors, simulates Cmd+V paste |
| `Sources/AudioRecorder.swift` | Records mic audio via AVAudioEngine, resamples to 16kHz mono |
| `Sources/WhisperTranscriber.swift` | Wraps whisper.cpp C API for transcription |
| `Sources/VAD.swift` | Simple RMS energy-based voice activity detection |
| `Sources/Refiner.swift` | Optional text cleanup via Ollama HTTP API |
| `Sources/Config.swift` | JSON config load/save, defaults |
| `config.py` | Python config (legacy) |
| `main.py` | Python prototype (legacy) |
| `refiner.py` | Python refiner (legacy) |
| `build.sh` | Builds whisper.cpp, compiles Swift, bundles .app, ad-hoc signs |
| `bridging-header.h` | Exposes whisper.cpp C API to Swift |

### Flow

```
 Hold Option  +--------------+    +----------+    +-------------+    +----------+    +-----------+
 ---------->  |  AVAudio     | -> |  VAD     | -> |  whisper.cpp | -> |  refiner | -> | clipboard |
 Release key  |  Engine      |    |  (RMS)   |    |  (base.en)   |    | (Ollama) |    | + Cmd+V   |
              +--------------+    +----------+    +-------------+    +----------+    +-----------+
              mic starts/stops    skip if silent                     skip if off    paste into focused app

 +------------------+
 | Menu bar icon    |  grey = idle, red = recording
 | (NSStatusItem)   |  right-click: status, Show Window, Quit
 +------------------+

 +------------------+
 | Settings window  |  model, language, VAD threshold, refiner toggle
 | (NSWindow)       |  output log box shows pipeline diagnostics
 +------------------+
```

### Threading Model

- **Main thread:** NSApplication run loop, UI updates, key event monitoring (NSEvent monitors).
- **Background thread:** model loading at startup, audio processing pipeline after each recording.
- **Audio tap thread:** AVAudioEngine input tap callback appends resampled frames to a locked buffer.

### Startup Sequence

1. Builds window and menu bar icon on main thread.
2. Dispatches model loading to background thread:
   - Requests microphone permission (blocks until granted/denied).
   - Loads whisper.cpp model from app bundle Resources.
   - Optionally checks Ollama connection for refiner.
3. Starts keyboard monitoring on main thread (NSEvent global + local monitors for `.flagsChanged`).
4. Tracks last-focused non-SpeakEasy app via `NSWorkspace.didActivateApplicationNotification`.

### Recording Cycle

1. User holds Option key — `onPushToTalkStart` fires.
2. `AVAudioEngine` starts with an input tap. Menu bar icon turns red.
3. User releases Option — `onPushToTalkEnd` fires.
4. Engine stops, captured buffer is copied out.
5. Background thread runs the pipeline: VAD check → Whisper transcription → optional Ollama refinement.
6. Result is placed on the system clipboard, the previous app is activated, and Cmd+V is simulated via CGEvent (with `nil` event source to avoid stale modifier state).

### Output Method

Uses **clipboard + simulated Cmd+V paste** rather than character-by-character CGEvent typing. This is more reliable because:
- Avoids stale modifier key state from the push-to-talk key (Option) bleeding into typed characters.
- Works consistently across all apps that support paste.
- Requires Accessibility permission in System Settings.

The CGEvent source is set to `nil` (not `.combinedSessionState`) to prevent the just-released Option key from contaminating the paste shortcut.

### Permissions Required

| Permission | Why | Where to grant |
|------------|-----|----------------|
| Microphone | AVAudioEngine needs mic access; without it, audio buffers are all zeros | System Settings > Privacy & Security > Microphone |
| Accessibility | CGEvent posting for simulated Cmd+V paste | System Settings > Privacy & Security > Accessibility |

**Important:** After rebuilding the app (which changes the ad-hoc code signature), both permissions may need to be re-granted. Reset with `tccutil reset Microphone com.speakeasy.app` and `tccutil reset Accessibility com.speakeasy.app`, then relaunch via `open SpeakEasy.app`.

### Shutdown

- **Quit menu item:** stops key monitor, shuts down whisper context, terminates app.
- **Window close:** app continues running in menu bar (`applicationShouldTerminateAfterLastWindowClosed` returns false).

### Refiner (`Sources/Refiner.swift`)

- Uses a local LLM via **Ollama** HTTP API (default model: `gemma3`).
- Checks connectivity via `GET /api/tags` with 3-second timeout.
- Sends chat requests to `POST /api/chat` with system prompt + rolling context.
- Maintains a rolling context of the last 5 refined sentences for coherence.
- Returns `nil` for pure filler (model outputs "SKIP"), falling back gracefully.

### Key Parameters

| Parameter        | Value        | Location              |
|------------------|--------------|-----------------------|
| Push-to-talk key | Option (any) | `KeyMonitor.swift`    |
| Sample rate      | 16000 Hz     | `AudioRecorder.swift` |
| VAD threshold    | 0.003 (RMS)  | `Config.swift`        |
| Max record time  | 30s          | `Config.swift`        |
| Whisper model    | base.en      | `Config.swift`        |
| Refiner model    | gemma3       | `Config.swift`        |
| Context window   | 5 sentences  | `Refiner.swift`       |

### Building

```bash
./build.sh    # builds whisper.cpp (if needed), compiles Swift, bundles .app, ad-hoc signs
open SpeakEasy.app   # must use `open` for permission dialogs to appear
```

Requires: Xcode command line tools, CMake (for whisper.cpp), `models/ggml-base.en.bin` model file.

## Legacy Python Prototype

The original Python version (`main.py` + `refiner.py`) uses faster-whisper, silero-vad, pynput, and pystray. It still works but is no longer the primary app.

```
python main.py                       # types refined text (uses gemma3 via Ollama)
python main.py --raw                 # types raw transcription (no refinement)
python main.py --no-type             # prints to console instead of typing
```
