# Release: memory-foundation

## 概要

AI 後処理（Claude）のたびに LLM が「覚えておくべき」と判断した情報を自動蓄積し、次回以降の処理精度を向上させる基盤。ユーザーは何もしなくても使うほどに精度が上がる体験を提供する。

## 背景

現在の koe は毎回の音声入力をゼロからのコンテキストで処理している。AI 後処理で修正した知識（用語、ユーザーの仕事内容、ドメイン知識）が次回に活かされない。

## 設計判断（/dig で決定済み）

| 項目 | 決定 |
|------|------|
| 抽出方式 | Claude Tool Use (function calling) |
| ツール設計 | `learn_term(from, to)` + `learn_context(category, content)` |
| データ形式 | ハイブリッド: 用語=構造化 TOML、コンテキスト=自由テキスト Markdown |
| 保存場所 | `~/.local/share/koe/memory/` |
| Ollama 対応 | 書き込みは Claude のみ、読み出し（プロンプト注入）は両方 |
| 学習頻度 | LLM が必要と判断したときだけ |
| カテゴリ | 事前定義 (user_profile, domain, project, workflow) + other |

## スコープ

### 含める

1. **Memory モジュール** (`src/memory/`)
   - `mod.rs` — Memory struct: 蓄積データのロード・保存・フォーマット
   - `terms.toml` の読み書き（構造化用語辞書）
   - `context.md` の読み書き（自由テキストコンテキスト）

2. **TextProcessor trait の拡張**
   - 戻り値を `String` → `ProcessResult { text: String, learnings: Vec<Learning> }` に変更
   - `Learning` enum: `Term { from, to }` / `Context { category, content }`

3. **Claude Tool Use 実装** (`src/ai/claude.rs`)
   - `learn_term` ツール定義: `{ from: string, to: string }`
   - `learn_context` ツール定義: `{ category: string, content: string }`
     - category: `user_profile` | `domain` | `project` | `workflow` | `other`
   - system prompt に学習指示を追加
   - tool_use レスポンスのパース

4. **Daemon 統合** (`src/daemon.rs`)
   - AI 処理後に learnings を Memory に保存
   - AI プロンプト生成時に Memory の蓄積データを注入
   - 起動時に Memory をロード
   - IPC ReloadConfig 時に Memory もリロード

5. **Ollama 側の読み出し対応** (`src/ai/ollama.rs`)
   - プロンプト注入のみ（tool_use は使わない）
   - `build_system_prompt` に memory 情報を含める

### 含めない（後続リリース）

- Whisper `initial_prompt` への注入 → R2
- 蓄積データの統合処理（LLM による要約） → R2
- 設定画面での閲覧・編集 UI → R3
- 既存の `dictionary.rs` との統合（memory が dictionary を置き換えるわけではない）

## ファイル構成

```
~/.local/share/koe/memory/
├── terms.toml      # 自動学習した用語辞書
└── context.md      # コンテキスト情報（自由テキスト）
```

### terms.toml フォーマット

```toml
# 自動学習された用語（LLM が learn_term で蓄積）
[terms]
"ラスト" = "Rust"
"クロード" = "Claude"
"コエ" = "koe"
```

### context.md フォーマット

```markdown
## user_profile
- Rust エンジニア。koe という音声入力ツールを開発している。

## domain
- ソフトウェア開発、Linux デスクトップ環境

## project
- koe: Rust製の音声入力ツール。Whisper + Claude で音声をテキストに変換。

## workflow
- コードを書きながら音声でコメントやドキュメントを入力することが多い。
```

## Tool Use 設計

### system prompt への追記

```
You have access to learning tools. When you notice information worth remembering
for future voice inputs (new terms, user context, domain knowledge), use the
appropriate tool. Only learn genuinely useful information — do not learn from
every input.

Available tools:
- learn_term: Record a term correction (e.g., misrecognized word → correct form)
- learn_context: Record contextual information about the user or their work
```

### learn_term ツール

```json
{
  "name": "learn_term",
  "description": "Record a term that was misrecognized by speech-to-text. Use when you correct a specific word or phrase that the user likely uses regularly.",
  "input_schema": {
    "type": "object",
    "properties": {
      "from": { "type": "string", "description": "The misrecognized form (e.g., 'ラスト')" },
      "to": { "type": "string", "description": "The correct form (e.g., 'Rust')" }
    },
    "required": ["from", "to"]
  }
}
```

### learn_context ツール

```json
{
  "name": "learn_context",
  "description": "Record contextual information about the user, their work, or domain. Use when you discover something that would help process future voice inputs more accurately.",
  "input_schema": {
    "type": "object",
    "properties": {
      "category": {
        "type": "string",
        "enum": ["user_profile", "domain", "project", "workflow", "other"],
        "description": "Category of the context information"
      },
      "content": {
        "type": "string",
        "description": "The information to remember (natural language, concise)"
      }
    },
    "required": ["category", "content"]
  }
}
```

## 変更対象ファイル

| ファイル | 変更内容 |
|---------|---------|
| `src/memory/mod.rs` | **新規**: Memory struct、ロード/保存/フォーマット |
| `src/ai/mod.rs` | `TextProcessor` trait 変更、`ProcessResult` 定義、`build_system_prompt` に memory 引数追加 |
| `src/ai/claude.rs` | Tool Use 対応: ツール定義、リクエスト構築、レスポンスパース |
| `src/ai/ollama.rs` | `process()` の戻り値変更対応（learnings は空）、プロンプト注入 |
| `src/daemon.rs` | Memory のロード・保存・注入の統合 |
| `src/main.rs` | `mod memory` 追加 |
| `src/config.rs` | `MemoryConfig` 追加（memory_dir のパス設定、enabled フラグ） |

## テスト計画

1. **Memory ロード/保存**: terms.toml と context.md の読み書きラウンドトリップ
2. **Tool Use パース**: Claude レスポンスから learnings を正しく抽出
3. **プロンプト注入**: Memory の内容が system prompt に正しくフォーマットされる
4. **重複排除**: 同じ term を2回学習しても1エントリ
5. **Ollama フォールバック**: Ollama 使用時に learnings が空で返る
6. **Memory なし起動**: memory ディレクトリが存在しない場合も正常起動

## リスク

- **Tool Use のレイテンシ**: tool_use は通常応答より若干遅い可能性。実使用で検証が必要
- **学習品質**: LLM が不適切な情報を蓄積するリスク。プロンプトの調整が必要かもしれない
- **API 互換性**: Claude Messages API の tool_use 形式が変わるリスク（低い）
