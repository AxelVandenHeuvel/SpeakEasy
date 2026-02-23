import os
import sys
from anthropic import Anthropic

SYSTEM_PROMPT = (
    "You are a text refinement assistant. You receive raw speech-to-text "
    "transcription and clean it into polished, coherent text. Fix grammar, "
    "remove filler words (um, uh, like, you know, so, basically), and "
    "restructure sentences for clarity. Preserve the speaker's meaning and "
    "tone exactly. Do not add anything they did not say. Output ONLY the "
    "refined text with no preamble or explanation. If the input is ONLY "
    "filler words or nonsense with no real content, output exactly: SKIP"
)

MAX_CONTEXT = 5  # rolling context of last N refined sentences


class Refiner:
    def __init__(self):
        api_key = os.environ.get("ANTHROPIC_API_KEY")
        if not api_key:
            print("[SpeakEasy] Error: ANTHROPIC_API_KEY environment variable not set.")
            sys.exit(1)
        self.client = Anthropic(api_key=api_key)
        self.context = []

    def refine(self, raw_text):
        context_str = " ".join(self.context)
        prompt = raw_text
        if context_str:
            prompt = f"Recent context: {context_str}\n\nNew transcription to refine: {raw_text}"

        message = self.client.messages.create(
            model="claude-haiku-4-5-20251001",
            max_tokens=256,
            system=SYSTEM_PROMPT,
            messages=[{"role": "user", "content": prompt}],
        )

        refined = message.content[0].text.strip()

        # Skip pure filler / no-content transcriptions
        if refined == "SKIP":
            return None

        # Update rolling context
        self.context.append(refined)
        if len(self.context) > MAX_CONTEXT:
            self.context.pop(0)

        return refined
