# Release: transcription-history

## 概要

音声入力の文字起こし結果を履歴として保存し、設定ウィンドウから一覧表示・検索・コピー・エクスポート・削除できる機能を追加する。

## 前提

- v0.3.0 (user-feedback) が完了済み
- daemon.rs に文字起こしフロー（raw_text → AI後処理 → タイピング）が存在
- GTK4 / libadwaita の設定ウィンドウ（PreferencesWindow）が存在
- memory モジュールが `~/.local/share/koe/memory/` にファイルベースで保存するパターンが確立済み

## スコープ

### 含める

1. **履歴データモジュール (`src/history/`)**
   - `HistoryEntry` 構造体: タイムスタンプ、生テキスト、AI後処理テキスト
   - JSONL 形式で `~/.local/share/koe/history/history.jsonl` に保存
   - 件数制限（デフォルト 1000 件）、超過時は古いエントリから自動削除
   - `add_entry()`, `search()`, `delete_entry()`, `clear()`, `export()` メソッド
   - テキスト検索（部分一致）、日付範囲フィルタ

2. **daemon 統合**
   - 文字起こし完了時に履歴エントリを自動保存
   - daemon.rs の Processing → Typing 遷移時に `history.add_entry()` を呼ぶ
   - config の `history.enabled` で有効/無効切替

3. **設定ウィンドウ「履歴」タブ**
   - PreferencesWindow に「履歴」ページを追加
   - 一覧表示: リスト形式で新しい順に表示（タイムスタンプ + テキストプレビュー）
   - テキスト検索バー: キーワードで絞り込み
   - 日付フィルタ: 日付範囲での絞り込み
   - コピーボタン: AI後処理テキストをクリップボードにコピー
   - 個別削除: 各エントリの削除ボタン
   - 一括クリア: 全履歴を削除するボタン（確認ダイアログ付き）

4. **エクスポート機能**
   - CSV / JSON 形式でファイルに書き出し
   - GTK ファイル選択ダイアログで保存先を指定

### 含めない

- 音声ファイルの保存 → 将来
- 履歴エントリの編集 → 将来
- ウィンドウコンテキスト（アプリ名等）の記録 → 将来
- IPC 経由での履歴アクセス → 将来
- 全文検索インデックス（JSONL の線形検索で十分）

## 変更対象ファイル

| ファイル | 変更内容 |
|---------|---------|
| `src/history/mod.rs` | **新規**: History 構造体、HistoryEntry、CRUD 操作、検索、エクスポート |
| `src/ui/history_page.rs` | **新規**: 履歴ページ UI（検索、一覧、コピー、削除、エクスポート） |
| `src/ui/settings_window.rs` | 履歴ページを PreferencesWindow に追加 |
| `src/ui/mod.rs` | history_page モジュール追加 |
| `src/daemon.rs` | 文字起こし完了時に history.add_entry() 呼び出し |
| `src/config.rs` | `HistoryConfig` 追加（enabled, dir, max_entries） |
| `src/main.rs` | `mod history;` 追加 |
| `Cargo.toml` | chrono 依存追加（タイムスタンプ）、バージョンを 0.4.0 に |

## 設計詳細

### データ構造

```rust
// src/history/mod.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,           // UUID v4
    pub timestamp: DateTime<Utc>,
    pub raw_text: String,
    pub processed_text: String,
}

pub struct History {
    entries: Vec<HistoryEntry>,
    dir: PathBuf,
    max_entries: usize,
}
```

### JSONL 形式

```jsonl
{"id":"550e8400-...","timestamp":"2026-03-04T14:30:45Z","raw_text":"あしたかいぎで...","processed_text":"明日の会議で..."}
{"id":"6ba7b810-...","timestamp":"2026-03-04T14:35:12Z","raw_text":"らすとの...","processed_text":"Rustの..."}
```

### History API

```rust
impl History {
    pub fn load(dir: &Path, max_entries: usize) -> Result<Self>;
    pub fn save(&self) -> Result<()>;
    pub fn add_entry(&mut self, raw_text: &str, processed_text: &str) -> Result<()>;
    pub fn search(&self, query: &str, from: Option<DateTime<Utc>>, to: Option<DateTime<Utc>>) -> Vec<&HistoryEntry>;
    pub fn delete_entry(&mut self, id: &str) -> Result<()>;
    pub fn clear(&mut self) -> Result<()>;
    pub fn export_csv(&self, path: &Path) -> Result<()>;
    pub fn export_json(&self, path: &Path) -> Result<()>;
}
```

### daemon.rs 統合

```rust
// Processing → Typing 遷移時（既存の文字起こし完了箇所）
if config.history.enabled {
    history.add_entry(&raw_text, &result.text)?;
}
```

### 設定

```toml
[history]
enabled = true
dir = "~/.local/share/koe/history"
max_entries = 1000
```

### UI レイアウト（履歴ページ）

```
┌─────────────────────────────────────┐
│ [🔍 検索...]  [日付: From] [To]     │
├─────────────────────────────────────┤
│ 2026-03-04 14:35                    │
│ 明日の会議でRustの説明があります。    │
│                        [コピー] [✕] │
├─────────────────────────────────────┤
│ 2026-03-04 14:30                    │
│ テスト音声入力です。                 │
│                        [コピー] [✕] │
├─────────────────────────────────────┤
│ ...                                 │
└─────────────────────────────────────┘
│ [エクスポート ▾]    [全履歴をクリア]  │
└─────────────────────────────────────┘
```

## テスト計画

1. **history/mod.rs**: add_entry, search, delete, clear, export のユニットテスト
2. **history/mod.rs**: 件数制限（max_entries 超過時の自動削除）テスト
3. **history/mod.rs**: JSONL の読み書き正確性テスト
4. **config.rs**: HistoryConfig のデフォルト値とシリアライズ
5. **統合テスト**: 既存テスト全件がパスすること
