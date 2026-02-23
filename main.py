import argparse
import signal
import threading
import time
import AppKit
import sounddevice as sd
import numpy as np
import torch
from PIL import Image, ImageDraw
from pystray import Icon, Menu, MenuItem
from faster_whisper import WhisperModel
from pynput.keyboard import Key, Listener, Controller as KeyboardController
from refiner import Refiner

SAMPLE_RATE = 16000
VAD_THRESHOLD = 0.25
PUSH_TO_TALK_KEYS = {Key.alt_r, Key.alt_l, Key.alt}
MAX_RECORD_SECONDS = 30


def create_icon_image(color):
    """Create a simple circle icon for the menu bar."""
    size = 64
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    draw.ellipse([8, 8, size - 8, size - 8], fill=color)
    return img


def load_vad():
    model, utils = torch.hub.load(
        repo_or_dir="snakers4/silero-vad", model="silero_vad", trust_repo=True
    )
    return model


def has_speech(vad_model, audio):
    """Check if audio chunk contains speech using silero-vad."""
    vad_model.reset_states()
    audio_tensor = torch.from_numpy(audio)
    window_size = 512
    for i in range(0, len(audio_tensor) - window_size, window_size):
        chunk = audio_tensor[i : i + window_size]
        speech_prob = vad_model(chunk, SAMPLE_RATE).item()
        if speech_prob > VAD_THRESHOLD:
            return True
    return False


def main():
    parser = argparse.ArgumentParser(description="SpeakEasy - local speech-to-text")
    parser.add_argument("--raw", action="store_true", help="Skip AI refinement")
    parser.add_argument("--no-type", action="store_true", help="Print to console instead of typing")
    args = parser.parse_args()

    typer = None
    if not args.no_type:
        typer = KeyboardController()

    print("[SpeakEasy] Loading VAD model...")
    vad_model = load_vad()
    print("[SpeakEasy] Loading Whisper model...")
    whisper = WhisperModel("base", device="cpu", compute_type="int8")

    refiner = None
    if not args.raw:
        print("[SpeakEasy] Initializing refiner...")
        refiner = Refiner()

    # Shared state
    recording = threading.Event()
    running = threading.Event()
    running.set()
    frames = []
    frames_lock = threading.Lock()
    stream_ref = [None]
    tray_icon_ref = [None]

    def audio_callback(indata, frame_count, time_info, status):
        with frames_lock:
            frames.append(indata.copy())

    def set_tray_state(state):
        """Update the tray icon color: 'idle' = grey, 'recording' = red."""
        icon = tray_icon_ref[0]
        if icon is None:
            return
        if state == "recording":
            icon.icon = create_icon_image("red")
        else:
            icon.icon = create_icon_image("gray")

    def on_press(key):
        if key in PUSH_TO_TALK_KEYS and not recording.is_set():
            with frames_lock:
                frames.clear()
            stream_ref[0] = sd.InputStream(
                samplerate=SAMPLE_RATE, channels=1, dtype="float32",
                callback=audio_callback, blocksize=int(SAMPLE_RATE * 0.1)
            )
            stream_ref[0].start()
            recording.set()
            set_tray_state("recording")

    def on_release(key):
        if key in PUSH_TO_TALK_KEYS and recording.is_set():
            recording.clear()
            if stream_ref[0] is not None:
                stream_ref[0].stop()
                stream_ref[0].close()
                stream_ref[0] = None
            set_tray_state("idle")

    def stop_stream():
        if stream_ref[0] is not None:
            stream_ref[0].stop()
            stream_ref[0].close()
            stream_ref[0] = None

    def quit_app(icon, item):
        running.clear()
        recording.clear()
        stop_stream()
        icon.stop()

    def signal_handler(sig, frame):
        print("\n[SpeakEasy] Stopped.")
        running.clear()
        recording.clear()
        stop_stream()
        if tray_icon_ref[0] is not None:
            tray_icon_ref[0].stop()

    signal.signal(signal.SIGINT, signal_handler)

    def get_status_text(item):
        return "Recording..." if recording.is_set() else "Idle"

    # Recording loop — runs on worker thread
    def run_loop():
        listener = Listener(on_press=on_press, on_release=on_release)
        listener.daemon = True
        listener.start()

        print("[SpeakEasy] Ready. Hold Option to talk.")

        try:
            while running.is_set():
                if not recording.wait(timeout=0.5):
                    continue

                start_time = time.time()
                while recording.is_set() and running.is_set():
                    time.sleep(0.05)
                    if (time.time() - start_time) >= MAX_RECORD_SECONDS:
                        print("[SpeakEasy] Safety timeout: auto-stopped after 30s")
                        recording.clear()
                        stop_stream()
                        set_tray_state("idle")
                        break

                with frames_lock:
                    captured = list(frames)
                    frames.clear()

                if not captured:
                    continue

                audio = np.concatenate(captured).flatten()

                if not has_speech(vad_model, audio):
                    continue

                segments, _ = whisper.transcribe(audio, beam_size=1, language="en")
                text = " ".join(s.text for s in segments).strip()

                if not text:
                    continue

                if refiner:
                    output = refiner.refine(text)
                    if output is None:
                        continue
                else:
                    output = text

                if typer:
                    typer.type(output + " ")
                else:
                    if refiner:
                        print(f"  [raw] {text}")
                        print(f"  [refined] {output}")
                    else:
                        print(output)
        finally:
            stop_stream()
            listener.stop()

    # Start recording loop on worker thread
    worker = threading.Thread(target=run_loop, daemon=True)
    worker.start()

    # Hide Python icon from Dock (run as background agent)
    AppKit.NSApplication.sharedApplication().setActivationPolicy_(
        AppKit.NSApplicationActivationPolicyAccessory
    )

    # Main thread: run tray icon
    tray_icon = Icon(
        "SpeakEasy",
        icon=create_icon_image("gray"),
        menu=Menu(
            MenuItem(get_status_text, None, enabled=False),
            MenuItem("Quit", quit_app),
        ),
    )
    tray_icon_ref[0] = tray_icon
    tray_icon.run()

    # After tray exits, ensure worker stops
    running.clear()
    worker.join(timeout=3)
    print("[SpeakEasy] Stopped.")


if __name__ == "__main__":
    main()
