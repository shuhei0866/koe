# koe - Project Instructions

## Versioning

セマンティックバージョニング (semver) で管理する。

- バージョンは `Cargo.toml` の `version` フィールドが source of truth
- `koe --version` で確認可能（clap の `version` 属性）
- main にマージする PR で機能追加・変更がある場合、`Cargo.toml` のバージョンを更新すること
  - 機能追加: minor バージョンを上げる (e.g., 0.2.0 → 0.3.0)
  - バグ修正のみ: patch バージョンを上げる (e.g., 0.2.0 → 0.2.1)
- マージ後に `git tag v<version>` でタグを打つ
- リファクタリングのみ・CI/設定変更のみの PR ではバージョンを上げなくてよい

## Build & Test

```bash
cargo check          # 型チェック
cargo test           # テスト実行
cargo build --release  # リリースビルド
```

- Rust ファイルを変更した後は `cargo check` を実行して型エラーがないことを確認する
- PR 作成前・コミット前は `cargo test && cargo clippy` も実行する
- デーモン関連の変更時は、重複起動防止ロジック（PID ファイル or D-Bus 名）が壊れていないか確認する

## Architecture

- `src/daemon.rs` — メインイベントループ（Idle → Recording → Processing → Typing）
- `src/recognition/` — 音声認識（Whisper local / OpenAI API）
- `src/ai/` — AI 後処理（Claude / Ollama）
- `src/memory/` — 自動学習メモリ（用語辞書 + コンテキスト）
- `src/config.rs` — 設定管理（`~/.config/koe/config.toml`）
- `src/ui/` — GTK4/libadwaita 設定 UI（`--features gui`）

## Worktree Convention

- メインワークツリーでは直接編集しない（フックでブロック）
- `git worktree add .worktrees/<name> -b <branch>` で作業用ワークツリーを作成
- ブランチ命名: `feature/<name>`, `fix/<name>`, `chore/<name>`, `release/<name>`
