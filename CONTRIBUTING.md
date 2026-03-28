# Contributing to koe

Thank you for your interest in contributing to koe! This document covers the basics for getting started.

## Getting Started

### Prerequisites

- Rust toolchain (install via [rustup](https://rustup.rs/))
- Linux (Ubuntu recommended, X11 required)
- System dependencies:

```bash
sudo apt install libasound2-dev libclang-dev libxkbcommon-dev \
  libx11-dev libxi-dev libxext-dev libxtst-dev libxfixes-dev cmake \
  libgtk-4-dev libadwaita-1-dev libvulkan-dev
```

### Build & Verify

```bash
cargo check        # Quick compilation check
cargo test         # Run tests
cargo clippy       # Lint
cargo build --release  # Release build
```

## Development Workflow

1. **Create a worktree and branch:**

   ```bash
   git worktree add .worktrees/<name> -b <prefix>/<name>
   ```

   Branch prefixes: `feature/`, `fix/`, `chore/`

2. **Implement your changes** in the worktree.

3. **Run checks before committing:**

   ```bash
   cargo fmt --check
   cargo clippy -- -D warnings
   cargo test
   ```

4. **Open a Pull Request** against `main`.

## Code Style

- Run `cargo fmt` before committing.
- Run `cargo clippy` and resolve all warnings.
- Keep functions focused and small.

## Commit Messages

We recommend [Conventional Commits](https://www.conventionalcommits.org/):

```text
feat: add new keybinding for toggle
fix: correct audio device detection on PipeWire
chore: update dependencies
```

## Pull Request Guidelines

- One logical change per PR.
- Include a short description of **what** and **why**.
- Ensure all CI checks pass (`cargo check`, `cargo test`, `cargo clippy`, `cargo fmt --check`).
- Link related issues if applicable.

## Issue Reporting

- Search existing issues before opening a new one.
- Include: OS/distro version, Rust version, steps to reproduce, expected vs actual behavior.
- Logs and error output are always helpful.

## Areas Where Help is Wanted

- **Wayland support** -- reducing X11 dependency
- **Packaging** -- deb, rpm, AUR, Nix, etc.
- **Multi-language dictionaries** -- expanding language coverage
- **Documentation** -- usage guides, examples, translations

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
