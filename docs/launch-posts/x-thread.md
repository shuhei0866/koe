# X (Twitter) Launch Thread

---

**1/7 - Hook**

Linux has no SuperWhisper. No Aqua Voice. No good AI voice input at all.

So I built one.

Introducing Koe -- AI-powered voice input for Linux. Whisper + LLM post-processing, context-aware, and it can run 100% locally.

https://github.com/shuhei0866/koe

---

**2/7 - Context Awareness**

The killer feature: Koe knows what app you're using.

Dictating in a terminal? It formats as commands.
Writing an email? Proper punctuation and tone.
Coding? It understands variable names and comments.

It reads your active window and adapts the output. This is what makes voice input actually usable.

---

**3/7 - AI Post-Processing**

Raw Whisper transcription is messy. Everyone knows this.

Koe pipes Whisper output through an LLM (Claude or Ollama) that cleans it up using your window context.

"open bracket let x equals five close bracket" becomes `let x = 5;` when it sees you're in an editor.

Plus custom dictionaries for your domain terms.

---

**4/7 - Tech Stack**

Built in Rust for low latency:

- whisper-rs (whisper.cpp bindings) for STT
- tokio async pipeline
- GTK4 native UI
- x11rb for window detection (X11)
- PulseAudio/PipeWire audio capture

Single binary. No Python runtime. No Electron.

---

**5/7 - Fully Local**

The whole thing can run without internet:

- whisper-rs for local transcription
- Ollama for local AI post-processing

No API keys. No accounts. No audio leaving your machine. Ever.

Your voice data stays on your hardware.

---

**6/7 - Demo**

[DEMO VIDEO PLACEHOLDER]

<!-- Record: 30-60 second screen capture showing:
  1. Push-to-Talk activation
  2. Dictating into a text editor (show context-aware formatting)
  3. Dictating into a terminal (show command formatting)
  4. Settings UI briefly
  Suggested tool: OBS or peek for GIF -->

---

**7/7 - CTA**

Koe v0.4.1 is out now. MIT licensed.

If you've wanted good voice input on Linux, give it a try:
https://github.com/shuhei0866/koe

Star it if it looks useful. Open an issue if something breaks. PRs welcome.

Built for the Linux desktop. Built in Rust. Built to run locally.
