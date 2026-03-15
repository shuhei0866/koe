# Reddit Launch Posts

---

## r/linux

### Title: I built an AI-powered voice input tool for Linux because I was jealous of macOS users

If you've ever used SuperWhisper or Aqua Voice on a Mac, you know how good voice input can be when it's done right -- context-aware, AI-enhanced, and seamlessly integrated into your workflow. Then you go back to Linux and... there's basically nothing.

So I built **Koe** -- a voice input system for Linux that uses Whisper for speech-to-text and AI post-processing (Claude or Ollama) to intelligently format the output based on what application you're currently using.

**What makes it different from existing Linux voice tools:**

- **Context awareness**: Koe reads your active window (app name, title, surrounding text) and uses that to format the transcription. Dictating in a terminal gets different formatting than dictating in an email client.
- **AI post-processing**: Raw transcription output is cleaned up, punctuated, and formatted by an LLM. This is the thing that makes Mac voice tools feel "magical" and it's been completely missing on Linux.
- **Fully local option**: You can run everything on your machine with whisper-rs + Ollama. No accounts, no API keys, no data leaving your system.
- **Push-to-Talk and Toggle modes**
- **Dictionary management** for technical terms and domain vocabulary
- **GTK4 settings UI** that feels native

It works with PulseAudio and PipeWire, uses x11rb for active window detection (X11), and is packaged as a single Rust binary.

v0.4.1 is out, MIT licensed: https://github.com/shuhei0866/koe

I'd really appreciate feedback from daily Linux users. What would make this actually useful for your workflow?

---

## r/rust

### Title: Koe: AI-powered voice input for Linux, built in Rust (whisper-rs, GTK4, async pipeline)

I've been working on **Koe**, a voice input system for Linux that combines Whisper STT with LLM-based post-processing. Sharing it here because the architecture might be interesting to other Rustaceans, and I'd love feedback on some of the design decisions.

**Why Rust:**

Voice input is latency-sensitive. The pipeline is: audio capture -> VAD -> transcription -> AI post-processing -> text injection. Every stage needs to be fast and the whole thing needs to run concurrently without blocking the UI. Rust's async model (tokio) and ownership system made this surprisingly manageable.

**Crate highlights:**

- **whisper-rs** -- Bindings to whisper.cpp. Handles local STT with good performance. The API is a bit low-level but wrapping it in a clean async interface wasn't too bad.
- **gtk4-rs** -- Settings UI. GTK4's Rust bindings have gotten much better. Still some rough edges with the object model but overall pleasant to use.
- **x11rb** -- X11 protocol for active window detection (title, class, app name). Clean API for window property queries.
- **zbus** -- D-Bus for daemon IPC (e.g., audio level signals between daemon and indicator UI).
- **cpal** -- Cross-platform audio capture. Works well with PulseAudio and PipeWire backends.
- **tokio** -- Runtime for the async pipeline. Channels for communication between pipeline stages.

**Architecture:**

The core is an async pipeline with distinct stages connected by tokio channels. Audio capture runs in its own thread (cpal callbacks), feeds into a transcription stage (whisper-rs), which feeds into an optional AI post-processing stage (HTTP client to Claude API or local Ollama), and finally text injection into the active window.

Context awareness is handled by querying the active window via x11rb (X11 protocol) before each post-processing call, reading the window title, class, and app name.

**Interesting challenges:**

- Managing whisper-rs model lifetime across async boundaries (it's not Send). Solved with a dedicated thread and channel-based communication.
- GTK4 main loop vs tokio runtime coexistence. Ended up running GTK on the main thread and tokio in a background runtime.
- Text injection on Wayland is still a pain. Currently using multiple strategies with fallbacks.

v0.4.1, MIT licensed: https://github.com/shuhei0866/koe

Would especially appreciate feedback on the async pipeline architecture and any crate suggestions I might have missed.

---

## r/selfhosted

### Title: Koe - Self-hosted voice input with AI, runs 100% locally on Linux

I built **Koe** for anyone who wants good voice input on Linux without sending audio to the cloud.

**The fully local setup:**

1. Install Koe (single Rust binary)
2. Install Ollama and pull a model (e.g., llama3)
3. Download a Whisper model

That's it. You now have AI-powered voice input that:

- Transcribes speech locally via whisper.cpp (no OpenAI API)
- Post-processes with Ollama (no cloud LLM)
- Knows what app you're using and formats accordingly
- Manages custom dictionaries for your terminology
- Has a native GTK4 settings UI

**Zero data leaves your machine.** No accounts, no API keys, no telemetry.

**What "AI post-processing" means in practice:**

Raw Whisper output: "open terminal and type git status and then git push origin main"

After post-processing (when Koe sees you're in a terminal): `git status && git push origin main`

It reads your active window context and adjusts formatting, punctuation, and even terminology based on what you're doing.

**Requirements:**

- Linux with PulseAudio or PipeWire
- GTK4
- For local STT: enough RAM/VRAM for a Whisper model (base model ~1GB VRAM)
- For local post-processing: enough resources to run an Ollama model

**Optional cloud backends** if you prefer: OpenAI Whisper API for STT, Claude for post-processing. You can mix and match (e.g., local Whisper + cloud Claude, or cloud Whisper + local Ollama).

v0.4.1, MIT licensed, single binary: https://github.com/shuhei0866/koe

Docker packaging is not available yet but on the roadmap. Feedback welcome, especially from people who've tried other self-hosted voice solutions.
