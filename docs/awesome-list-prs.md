# Awesome List PR Preparation for koe

**Project:** [koe](https://github.com/shuhei0866/koe)
**Description:** Linux voice dictation in Rust with Whisper, AI post-processing, and active-window context awareness.
**License:** MIT | **Language:** Rust | **Current stars:** 0 (as of 2026-03-16)

---

## Target Lists

### 1. awesome-rust (rust-unofficial)

| Item | Detail |
|---|---|
| **Repository** | https://github.com/rust-unofficial/awesome-rust |
| **Stars** | ~50k |
| **Target section** | `Applications > Audio and Music` or `Applications > Utilities` or `Applications > Text processing` |
| **Requirements** | At least **50 GitHub stars** OR **2,000 crates.io downloads**, or equivalent popularity metric (must be stated in PR) |
| **Ordering** | Alphabetical within section |
| **Contributing guide** | https://github.com/rust-unofficial/awesome-rust/blob/main/CONTRIBUTING.md |

**Proposed entry:**

```markdown
* [shuhei0866/koe](https://github.com/shuhei0866/koe) - AI-powered voice dictation for Linux with Whisper, post-processing via Ollama, and active-window context awareness
```

If published to crates.io:
```markdown
* [shuhei0866/koe](https://github.com/shuhei0866/koe) [[koe](https://crates.io/crates/koe)] - AI-powered voice dictation for Linux with Whisper, post-processing via Ollama, and active-window context awareness
```

---

### 2. awesome-whisper (sindresorhus)

| Item | Detail |
|---|---|
| **Repository** | https://github.com/sindresorhus/awesome-whisper |
| **Stars** | ~1.5k |
| **Target section** | `Apps` (alongside Speech Note, Buzz, and other desktop apps) |
| **Requirements** | At least **20 GitHub stars**; open-source projects must link to GitHub repo; description must start with capital, end with period, no marketing language, no title-case |
| **Ordering** | Added to the **bottom** of the relevant category |
| **PR title format** | `Add koe` |
| **Contributing guide** | https://github.com/sindresorhus/awesome-whisper/blob/main/contributing.md |

**Proposed entry:**

```markdown
- [koe](https://github.com/shuhei0866/koe) - AI-powered voice dictation for Linux with active-window context awareness and local post-processing. (FOSS)
```

---

### 3. awesome-voice-typing (primaprashant)

| Item | Detail |
|---|---|
| **Repository** | https://github.com/primaprashant/awesome-voice-typing |
| **Stars** | ~200 |
| **Target section** | `Directory` table |
| **Requirements** | Open source, voice-typing/dictation focused, usable as a product (not a library), repository must be public |
| **Ordering** | Alphabetical (case-insensitive) in the Directory table |
| **PR title format** | `Add koe` |
| **Contributing guide** | https://github.com/primaprashant/awesome-voice-typing/blob/main/CONTRIBUTING.md |

**Proposed entry (table row):**

```markdown
| [koe](https://github.com/shuhei0866/koe) | Linux | Local | Whisper | AI-powered voice dictation for Linux with active-window context awareness and Ollama post-processing. |
```

Also add `koe` to the Linux line in the `Browse by platform` details block (alphabetical order).

---

### 4. Awesome-Linux-Software (luong-komorebi)

| Item | Detail |
|---|---|
| **Repository** | https://github.com/luong-komorebi/Awesome-Linux-Software |
| **Stars** | ~22k |
| **Target section** | `Applications > Audio > Utilities` or `Applications > Utilities > Other` |
| **Requirements** | No explicit star threshold; entry needs name, homepage/install link, short description, icon, alphabetical order |
| **Ordering** | Alphabetical within section |
| **Contributing guide** | https://github.com/luong-komorebi/Awesome-Linux-Software/blob/main/CONTRIBUTING.md |

**Proposed entry:**

```markdown
- [![Open-Source Software][oss icon]](https://github.com/shuhei0866/koe) [koe](https://github.com/shuhei0866/koe) - AI-powered voice dictation for Linux using Whisper with active-window context awareness and local post-processing via Ollama.
```

---

### 5. awesome-openai-whisper (ancs21)

| Item | Detail |
|---|---|
| **Repository** | https://github.com/ancs21/awesome-openai-whisper |
| **Stars** | ~500 |
| **Target section** | `Applications` |
| **Requirements** | No explicit threshold documented; simple bullet-point format |
| **Ordering** | No strict ordering observed |

**Proposed entry:**

```markdown
* [koe - AI-powered voice dictation for Linux](https://github.com/shuhei0866/koe)
```

---

## PR Creation Procedure

For each target list:

1. **Fork** the repository.
2. **Create a branch** named `add-koe`.
3. **Edit README.md** and add the entry in the correct section, following the format above.
4. **Commit** with message: `Add koe`.
5. **Open a PR** with the title `Add koe` (or `Add shuhei0866/koe` for awesome-rust).
6. In the PR description, briefly describe koe:
   - What it is (Linux voice dictation tool)
   - Key differentiators (Rust, active-window context, Ollama post-processing, privacy-focused local processing)
   - Link to the repo
   - Mention the license (MIT)

### PR Description Template

```markdown
## Add koe

[koe](https://github.com/shuhei0866/koe) is an AI-powered voice dictation tool for Linux, written in Rust.

**Key features:**
- Uses Whisper for local speech recognition
- Active-window context awareness for smarter post-processing
- AI post-processing via Ollama (runs entirely locally)
- Built in Rust for performance and reliability
- MIT licensed

**Popularity metrics:** [stars] GitHub stars (include current count at time of PR)
```

---

## Important Notes and Risks

### Star Count Barrier (Critical)

koe currently has **0 stars**. This is the biggest blocker:

| List | Minimum Stars | Status |
|---|---|---|
| awesome-rust | 50 stars (or 2,000 crates.io downloads) | **Blocked** |
| awesome-whisper | 20 stars | **Blocked** |
| awesome-voice-typing | No explicit minimum | **Eligible now** |
| Awesome-Linux-Software | No explicit minimum | **Eligible now** |
| awesome-openai-whisper | No explicit minimum | **Eligible now** |

### Recommended Submission Order

1. **Now:** Submit to **awesome-voice-typing** and **awesome-openai-whisper** (no star requirements).
2. **Now (with caution):** Submit to **Awesome-Linux-Software** (no explicit requirement, but very low star count may lead to rejection at maintainer discretion).
3. **After 20+ stars:** Submit to **awesome-whisper**.
4. **After 50+ stars:** Submit to **awesome-rust**.

### Other Considerations

- **Quality signals matter:** Before submitting, ensure the repo has a polished README, clear installation instructions, screenshots/GIFs of the tool in action, and CI badges. Maintainers often review the project page before merging.
- **One PR per list:** Each list requires a separate PR to a separate repo. Never batch.
- **Expect review feedback:** Maintainers may ask for wording changes, section moves, or format fixes. Be responsive.
- **Don't spam:** If a PR is rejected, do not resubmit immediately. Address feedback or wait until the project meets requirements.
- **awesome-whisper is strictly curated:** Maintained by sindresorhus, who is known for high standards. The 20-star minimum is a hard gate.
- **awesome-rust requires proof of popularity:** If using an alternative metric (not GitHub stars or crates.io downloads), you must explicitly state it in the PR.
- **Crates.io publication helps:** For awesome-rust, publishing to crates.io and accumulating downloads is an alternative path to the 50-star requirement.
