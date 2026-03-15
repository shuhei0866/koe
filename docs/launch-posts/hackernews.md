# Show HN: Koe - AI-powered voice input for Linux (Rust, Whisper, fully local option)

## Post Body

I built Koe because I was tired of the voice input gap between macOS and Linux. Tools like SuperWhisper and Aqua Voice offer great dictation experiences on Mac, but Linux had nothing comparable. So I built one.

**What it does:**

Koe is a voice input system for Linux/Ubuntu that transcribes speech and pipes it into whatever application you're using. It uses Whisper for STT (local via whisper-rs or cloud via OpenAI API) and optionally runs AI post-processing through Claude or Ollama to clean up and format the output.

The key differentiator is context awareness. Koe reads your active window information -- the application name, window title, and surrounding text -- and uses that context during AI post-processing. Dictating into a terminal? It formats as a command. Writing an email? It adjusts tone and punctuation. Coding in your editor? It understands you're probably dictating a comment or variable name.

**Features:**

- Push-to-Talk and Toggle modes
- Local-first: whisper-rs + Ollama means zero data leaves your machine
- Dictionary management for domain-specific vocabulary
- GTK4 settings UI
- AI post-processing with context from your active window
- Works with both local and cloud backends (mix and match)

**Tech stack:**

- Rust (entire application)
- whisper-rs for local STT (bindings to whisper.cpp)
- PulseAudio/PipeWire for audio capture
- GTK4 for the settings UI
- x11rb for active window detection (X11)
- D-Bus (zbus) for daemon IPC
- Claude API or Ollama for AI post-processing

**Why Rust?** Latency matters for voice input. You want transcription to feel instant. Rust gives predictable performance with low memory overhead, which is important when you're also running a Whisper model locally. The type system also helped a lot with managing the async pipeline (audio capture -> transcription -> post-processing -> text injection).

**Fully local setup:** Install Ollama, download a Whisper model, and you have a complete voice input system with AI post-processing that never phones home. No API keys needed.

v0.4.1 is out now. MIT licensed.

GitHub: https://github.com/shuhei0866/koe

I'd love feedback on the architecture, feature requests, or bug reports. Happy to answer questions about the implementation.

---

## FAQ (Anticipated HN Comments)

**Q: How does this compare to Nerd Dictation / other existing Linux voice tools?**
A: Most existing tools are thin wrappers around STT engines. Koe adds the AI post-processing layer with context awareness, which is what makes tools like SuperWhisper feel magical on macOS. Raw transcription vs. intelligent dictation is a big UX gap.

**Q: What's the latency like with local Whisper?**
A: Depends on your hardware and model size. With whisper-rs (whisper.cpp under the hood) and the base model on a decent GPU, you're looking at near-real-time. The small model on CPU-only is usable but noticeably slower. Cloud Whisper API is the fastest option if you don't mind the network round-trip.

**Q: Does it work on Wayland?**
A: Currently X11 only. Active window detection uses x11rb (X11 protocol), so Wayland is not supported yet. Wayland support is on the roadmap — contributions welcome. Audio capture via cpal works fine with PipeWire.

**Q: Why not just use the OpenAI Whisper Python library?**
A: Performance and integration. whisper-rs (whisper.cpp) is significantly faster for inference, and being in Rust means the whole pipeline from audio capture to text injection is a single binary with no Python runtime dependency.

**Q: How much VRAM does local Whisper need?**
A: The base model needs ~1GB VRAM. Small ~2GB. Medium ~5GB. You can also run on CPU-only, just slower. Ollama for post-processing adds its own requirements depending on the model you choose.

**Q: Privacy -- what data is sent where?**
A: In fully local mode (whisper-rs + Ollama): nothing leaves your machine. If you use cloud Whisper API, audio goes to OpenAI. If you use Claude for post-processing, the transcribed text + window context goes to Anthropic. You pick your comfort level.

**Q: Will this work on Fedora/Arch/etc.?**
A: It's primarily tested on Ubuntu but should work on any Linux distro with PulseAudio or PipeWire and GTK4. Packaging for other distros is on the roadmap.
