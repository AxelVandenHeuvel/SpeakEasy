# SpeakEasy

Local, real-time speech-to-text that types into any window. Hold a key, speak, release — your words appear wherever your cursor is, optionally refined by AI.

## How It Works

1. Hold the **Option** key and speak
2. Release the key
3. SpeakEasy transcribes your speech locally using [Whisper](https://github.com/openai/whisper)
4. Optionally refines the text with Claude Haiku (fixes grammar, removes filler words)
5. Types the result into whatever window is focused

A menu bar icon shows grey (idle) or red (recording).

For full technical details, see [ARCHITECTURE.md](ARCHITECTURE.md).

## Requirements

- macOS (tested on macOS 15+)
- Python 3.10+
- Microphone access (macOS will prompt on first run)
- Accessibility permissions for keyboard input (System Settings > Privacy & Security > Accessibility)
- [Anthropic API key](https://console.anthropic.com/settings/keys) (for AI text refinement — optional if using `--raw` mode)

## Quick Start

```bash
# Clone the repo
git clone https://github.com/AxelVandenHeuvel/SpeakEasy.git
cd SpeakEasy

# Create and activate virtual environment
python3 -m venv venv
source venv/bin/activate

# Install dependencies
pip install -r requirements.txt

# Set your Anthropic API key (get one at https://console.anthropic.com/settings/keys)
export ANTHROPIC_API_KEY="your-key-here"

# Run
python3 main.py
```

> **BYOK (Bring Your Own Key):** SpeakEasy uses Claude Haiku for text refinement. You need your own Anthropic API key. Get one at [console.anthropic.com](https://console.anthropic.com/settings/keys). Copy `.env.example` to `.env` and add your key, or export it directly in your terminal.

## Usage

```bash
python3 main.py                  # AI-refined text typed into focused window
python3 main.py --raw            # Raw transcription (no AI refinement, no API key needed)
python3 main.py --no-type        # Print to console instead of typing
python3 main.py --raw --no-type  # Raw output to console only
```

### Push-to-Talk

- **Hold Option** to record
- **Release Option** to stop and transcribe
- Menu bar icon: grey = idle, red = recording
- Right-click the menu bar icon to quit

## Troubleshooting

### "Accessibility" permission error
Go to **System Settings > Privacy & Security > Accessibility** and add your terminal app (Terminal, iTerm2, etc.).

### Microphone not working
Go to **System Settings > Privacy & Security > Microphone** and ensure your terminal app has access.

### `ANTHROPIC_API_KEY not set` error
Export your key before running: `export ANTHROPIC_API_KEY="your-key-here"`. Or use `--raw` mode to skip AI refinement entirely.

### Ctrl+C not stopping the app
Right-click the menu bar icon and select **Quit**, or use `Ctrl+Z` then `kill %1`.

## License

[MIT](LICENSE)
