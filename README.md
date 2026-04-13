# SpeakEasy

Local, real-time speech-to-text for macOS. Hold a key, speak, release — your words appear wherever your cursor is, optionally refined by a local AI. Everything runs on your machine. No cloud, no API keys, no subscription.

## Download

**[Download for macOS (Apple Silicon)](https://github.com/AxelVandenHeuvel/SpeakEasy/releases/latest)**

Signed with Developer ID and notarized by Apple — opens without Gatekeeper warnings.

Windows build coming soon.

## How It Works

1. Hold **Option** (configurable) and speak
2. Release the key
3. SpeakEasy transcribes your speech locally using [whisper.cpp](https://github.com/ggerganov/whisper.cpp)
4. Optionally refines the text with a local Qwen 2.5 LLM via [llama.cpp](https://github.com/ggerganov/llama.cpp)
5. Pastes the result into whatever window is focused

A menu bar icon shows when it's listening.

## Features

- **Three refinement modes** — Raw, Clean (rule-based filler removal), or Rewrite (LLM restructures for clarity)
- **Two tones** — Normal (casual) or Formal (professional / emails)
- **Voice shortcuts** — say a trigger phrase, get custom text pasted (e.g., "my email" → `you@example.com`)
- **Personal dictionary** — help Whisper spell your names, brand names, technical terms correctly
- **Dictation commands** — "new line", "question mark", "delete that", etc.
- **Color themes** — Dusk Sprinter, Mono, Classic, Vaporwave
- **Custom push-to-talk key** — up to 3-key combos

## Requirements

- macOS 12 (Monterey) or later
- Apple Silicon (M1/M2/M3) — Intel not yet supported

## Privacy

Everything happens on your device. SpeakEasy does not send audio, text, or any data to the cloud. No accounts, no telemetry, no API calls.

## Building from Source

```bash
# Clone with submodules (whisper.cpp, llama.cpp)
git clone --recurse-submodules https://github.com/AxelVandenHeuvel/SpeakEasy.git
cd SpeakEasy

# Download models (~1.2GB)
mkdir -p models
curl -L -o models/ggml-base.en.bin https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
curl -L -o models/qwen2.5-1.5b-instruct-q4_k_m.gguf https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf

# Build the llama-server sidecar (required for LLM refinement)
cd vendor/llama.cpp
cmake -B build -DBUILD_SHARED_LIBS=OFF -DLLAMA_BUILD_SERVER=ON -DLLAMA_SERVER_SSL=OFF -DCMAKE_DISABLE_FIND_PACKAGE_OpenSSL=TRUE -DCMAKE_BUILD_TYPE=Release
cmake --build build --target llama-server -j$(sysctl -n hw.ncpu)
cp build/bin/llama-server ../../tauri-app/src-tauri/binaries/llama-server-aarch64-apple-darwin
cd ../..

# Run in dev mode
cd tauri-app
cargo tauri dev
```

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for technical details.

Tech stack:
- **Rust backend** via [Tauri](https://tauri.app/) — audio recording (cpal), hotkey monitoring (device_query), clipboard + paste (enigo/AppleScript)
- **HTML/CSS/JS frontend** — single-page app, no framework
- **whisper.cpp** — speech-to-text via Core ML / Metal
- **llama.cpp** — LLM refinement via Metal, as a bundled sidecar HTTP server

## License

[MIT](LICENSE)
