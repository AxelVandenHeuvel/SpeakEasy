# SpeakEasy Architecture

## Project Overview

SpeakEasy is a local, real-time speech-to-text tool inspired by WhisperFlow. It records audio from your microphone and transcribes it using OpenAI's Whisper model running locally via faster-whisper. Transcriptions are optionally refined by Claude Haiku for polished output.

## Current Architecture (Phase 6)

Two files: `main.py` (recording loop + tray icon + keyboard output) and `refiner.py` (AI text cleanup).

### Flow

```
 Hold key     +--------------+    +-------------+    +----------------+    +----------+    +----------+
 --------->   |  Microphone  | -> |  silero-vad | -> |  faster-whisper | -> |  refiner | -> | keyboard |
 Option key   | (on-demand)  |    |  (filter)   |    |  (base, int8)  |    | (Haiku)  |    | (pynput) |
              +--------------+    +-------------+    +----------------+    +----------+    +----------+
 Release key    mic starts/stops    skip if silent     skip if --raw                     stdout if --no-type

 +------------------+
 | Menu bar icon    |  grey = idle, red = recording
 | (pystray/Pillow) |  right-click: status + Quit
 +------------------+
```

### Threading Model

- **Main thread:** runs `pystray` Icon event loop (required on macOS for AppKit integration).
- **Worker thread:** runs the recording/transcription loop and pynput keyboard listener.
- **Audio callback thread:** `sounddevice.InputStream` callback appends frames to a shared buffer.

### Startup Sequence

1. Loads silero-vad, faster-whisper `base` model, pynput keyboard controller, and (unless `--raw`) the Refiner.
2. Starts the worker thread (recording loop + keyboard listener).
3. Creates and runs the pystray menu bar icon on the main thread.

### Recording Cycle

1. Waits for the push-to-talk key (Option) to be held down.
2. On key press, opens a `sounddevice.InputStream` with a callback. Tray icon turns red.
3. On key release, stops and closes the stream. Tray icon returns to grey.
4. Concatenates frames, runs VAD, transcribes, optionally refines, types/prints output.
5. Returns to step 1.

### Shutdown

- **Quit menu item:** calls `icon.stop()` + `running.clear()`, exits both tray and worker thread.
- **Ctrl+C:** SIGINT handler does the same.
- **Safety timeout:** 30-second max recording duration auto-stops the mic.

### Audio Recording

Uses a **callback-based** `sounddevice.InputStream` (not blocking `stream.read()`). The stream is created on key press and destroyed on key release, so the macOS mic indicator only appears while recording. A threading lock protects the shared frames buffer.

### Refiner (`refiner.py`)

- Uses `claude-haiku-4-5-20251001` via the `anthropic` Python SDK.
- Reads API key from `ANTHROPIC_API_KEY` env var. Exits with error if missing.
- Maintains a rolling context of the last 5 refined sentences for coherence across chunks.
- System prompt instructs: fix grammar, remove filler words, preserve meaning, output only refined text.

### Key Parameters

| Parameter        | Value        | Location            |
|------------------|--------------|---------------------|
| Push-to-talk key | Option (any) | `PUSH_TO_TALK_KEYS` |
| Sample rate      | 16000 Hz     | `SAMPLE_RATE`       |
| Block size       | 100ms        | `blocksize`         |
| VAD threshold    | 0.25         | `VAD_THRESHOLD`     |
| VAD window       | 512          | `has_speech()`      |
| Max record time  | 30s          | `MAX_RECORD_SECONDS`|
| Model size       | base         | `WhisperModel()`    |
| Compute type     | int8         | `WhisperModel()`    |
| Beam size        | 1            | `model.transcribe()`|
| Language         | en           | `model.transcribe()`|
| Refiner model    | claude-haiku-4-5-20251001 | `refiner.py` |
| Context window   | 5 sentences  | `MAX_CONTEXT`       |

## Entry Point

```
python main.py                # types refined text into focused window
python main.py --raw          # types raw transcription (no AI refinement)
python main.py --no-type      # prints to console instead of typing
python main.py --raw --no-type  # raw output to console only
```

## Dependencies

| Package          | Purpose                                                      |
|------------------|--------------------------------------------------------------|
| `sounddevice`    | Cross-platform audio recording via PortAudio                 |
| `numpy`          | Audio data as NumPy arrays (required by sounddevice)         |
| `faster-whisper` | CTranslate2-based Whisper implementation for fast CPU inference |
| `torch`          | PyTorch runtime, required by silero-vad                        |
| `torchaudio`     | Audio utilities, required by silero-vad                        |
| `anthropic`      | Anthropic Python SDK for Claude Haiku API                      |
| `pynput`         | Simulates keyboard input to type into focused window           |
| `pystray`        | macOS menu bar icon                                            |
| `Pillow`         | Programmatic icon image creation                               |

## Future Phases (Roadmap)

- **Phase 7** -- CLI flags and configuration (--model, --language, --hotkey, etc.)
