# koe Parallel Development Skill

koe（Ubuntu 向け Rust 音声入力ツール）の開発を並列エージェントで効率化するスキル。

## プロジェクト概要

- リポジトリ: `~/koe`
- 言語: Rust (edition 2021)
- 主要依存: tokio, cpal, whisper-rs, rdev, enigo, x11rb, GTK4/libadwaita（設定 UI）
- 設定ファイル: `~/.config/koe/config.toml`

## モジュール構成

```
src/
├── main.rs          # エントリポイント
├── audio.rs         # マイク入力キャプチャ (cpal)
├── recognition/
│   ├── mod.rs       # 音声認識トレイト
│   ├── whisper_local.rs  # ローカル Whisper
│   └── openai_api.rs     # OpenAI API 経由
├── ai/
│   ├── mod.rs       # AI 後処理トレイト
│   ├── claude.rs    # Claude API
│   └── ollama.rs    # Ollama (ローカル)
├── context.rs       # アクティブウィンドウのコンテキスト取得
├── config.rs        # 設定ファイル読み書き
├── dictionary.rs    # ユーザー辞書
├── hotkey.rs        # グローバルホットキー (rdev)
└── input.rs         # テキスト入力シミュレーション (enigo)
```

## 並列ワークストリーム

koe の機能は以下の3つの独立したストリームに分解できる。各ストリームは別々のワークツリーで並列開発可能。

### Stream 1: Audio Pipeline（音声パイプライン）

**スコープ**: `audio.rs`, `recognition/`
**テストゲート**: `cargo test audio` / `cargo test recognition`

- cpal でのマイク入力キャプチャ
- whisper-rs によるローカル音声認識
- OpenAI API による音声認識
- WAV ファイルからの認識テスト

### Stream 2: Settings UI（設定画面）

**スコープ**: 新規 `src/ui/` モジュール
**テストゲート**: `cargo test ui` / `cargo test settings`
**依存**: GTK4, libadwaita

- GTK4 + libadwaita による設定ウィンドウ
- トグルスイッチ、ドロップダウン
- 設定の保存・読み込み（コールバックが正しく発火すること）
- XDG 準拠のファイルパス

### Stream 3: Config & Integration（設定永続化 & 統合）

**スコープ**: `config.rs`, `context.rs`, `dictionary.rs`
**テストゲート**: `cargo test config` / `cargo test context`

- serde による TOML シリアライゼーション
- スキーマバージョニング
- ラウンドトリップテスト（保存 → 読み込み → 比較）
- ウィンドウコンテキスト取得

## 並列開発の実行方法

```
# ワークツリーを3つ作成
cd ~/koe
git worktree add .worktrees/audio-pipeline -b feature/audio-pipeline
git worktree add .worktrees/settings-ui -b feature/settings-ui
git worktree add .worktrees/config-persist -b feature/config-persist
```

### エージェント起動テンプレート

各エージェントには以下のルールを適用:

1. `cargo check` を編集ごとに実行し、コンパイルエラーを即修正
2. 該当するテストスイートを最低3回実行して安定性を確認
3. 共有型は `src/types.rs` に定義（マージ時のコンフリクトを最小化）
4. 完了時にサマリーを報告: 何が動いて何が動かないか

### 合流（マージ）

3つのストリームが完了したら:
1. 共有型のコンフリクトを解決
2. `cargo check` → `cargo test` → `cargo build` の順で全体検証
3. main/develop にマージ

## 既知の注意点

- settings-ui ワークツリーが既に存在する（前回セッションから）
- GTK4 の設定保存コールバックが未解決の可能性あり（前回セッション末尾で問題報告）
- whisper-rs のビルドには libclang が必要（`sudo apt install libclang-dev`）
