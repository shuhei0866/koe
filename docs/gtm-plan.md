# Go-to-Market Plan: koe

**ローンチ目標日**: 2026-03-29（2週間後）
**タイプ**: 既存 OSS プロダクトの初回パブリックローンチ
**成功の定義**: OSS 界隈での認知度獲得、ユーザー採用、コミュニティ形成

---

## Beachhead Segment

### 候補セグメントの評価

| セグメント | 切実な課題 | 採用意欲 | 獲得可能性 | 拡散力 | 合計 |
|---|---|---|---|---|---|
| **A. Linux で音声入力したい開発者** | 5 | 5 | 4 | 5 | **19** |
| B. Rust エコシステムの開発者 | 3 | 4 | 3 | 5 | 15 |
| C. プライバシー重視のパワーユーザー | 4 | 3 | 3 | 3 | 13 |
| D. アクセシビリティを必要とするユーザー | 5 | 4 | 2 | 2 | 13 |
| E. セルフホスト愛好家 | 3 | 3 | 3 | 4 | 13 |

（各項目 1-5 点）

### 選定: A. Linux で音声入力が欲しい開発者

**Why them first**:

1. **切実な課題**: macOS には superwhisper、Aquavoice、Whisper Transcription がある。Windows には既成の選択肢がある。Linux にはまともな音声入力ツールが**ほぼ存在しない**。このペインは本物
2. **採用意欲**: 開発者はコマンドラインからビルドして即使える。セットアップの壁が低い
3. **獲得可能性**: この領域の競合 OSS はほぼゼロ。nerd-dictation（Python、AI後処理なし）が唯一の代替で、機能差が大きい
4. **拡散力**: 開発者は HackerNews、Reddit、X で積極的にツールを共有する文化がある。「Linux に足りなかったもの」は拡散しやすいテーマ

### 隣接セグメント（ビーチヘッド後の展開順）

1. Rust コミュニティ（技術スタックへの共感で拡散）
2. セルフホスト / プライバシー重視層（Ollama + ローカル Whisper の組み合わせが刺さる）
3. アクセシビリティ層（身体的に音声入力が必要なユーザー）
4. 非英語圏の Linux ユーザー（辞書機能 + 多言語 Whisper）

### 市場規模の参考値

- Linux デスクトップユーザー: 約 3,000 万人（StatCounter 推計）
- うち開発者で音声入力に関心あり: 推計 1-3%（30万-90万人）
- 初期ターゲット（英語/日本語、Ubuntu/Fedora）: 数万人規模
- **現実的初期目標**: 最初の100人のアクティブユーザー

---

## Ideal User Profile（OSS 版 ICP）

| 属性 | 定義 |
|---|---|
| OS | Ubuntu 22.04+ / Fedora / Arch（X11 環境） |
| 職種 | ソフトウェアエンジニア、テクニカルライター、研究者 |
| 技術レベル | ターミナルからビルドできる（cargo が使える or 学ぶ意欲がある） |
| Key JTBD | 「Linux で長文を書くとき、タイピングの代わりに声で入力したい」 |
| 現在の解決策 | 諦めている / Google Docs の音声入力を無理やり使っている / nerd-dictation |
| 不満 | macOS の音声入力ツールが羨ましい、Linux 版がない |
| 発見方法 | Reddit, HackerNews, GitHub Trending, X, ブログ記事 |
| 見分け方 | "linux voice input" "linux speech to text" で検索している人 |

---

## Positioning & Messaging

### ポジショニングステートメント

> **Linux で音声入力をしたい開発者**のために、**koe** は **Whisper + AI 後処理を組み合わせたオープンソースの音声入力システム**です。ホットキーひとつで録音し、AI がコンテキストを理解して賢く整形してくれます。macOS の superwhisper に相当するものを、Linux でオープンソースとして実現しました。

### キーメッセージ

| オーディエンス | メッセージ | 裏付け |
|---|---|---|
| Linux 開発者 | 「macOS には superwhisper がある。Linux には koe がある」 | 唯一の AI 後処理付き Linux 音声入力 OSS |
| Rust 開発者 | 「whisper.cpp + Rust で作られた、ハックしやすい音声入力」 | モジュラーなアーキテクチャ、MIT ライセンス |
| プライバシー重視層 | 「完全ローカル実行可能。音声データはどこにも送らない」 | whisper-rs + Ollama の組み合わせでクラウド不要 |
| セルフホスト層 | 「自分のマシンで動く、自分の AI で処理する音声入力」 | Ollama 統合、設定の柔軟性 |

### エレベーターピッチ（各チャネル用）

**HackerNews 用（技術寄り）**:
> Show HN: koe - AI-powered voice input for Linux (Rust, Whisper, open source)
>
> I built a voice input tool for Linux because nothing like superwhisper existed. koe uses Whisper for speech recognition and Claude/Ollama for context-aware post-processing. It reads your active window title to understand what you're working on and formats the text accordingly. Fully local mode available (whisper.cpp + Ollama). Written in Rust, MIT licensed.

**Reddit r/linux 用（ユーザー寄り）**:
> I made an open-source voice input system for Linux - basically superwhisper for Ubuntu
>
> macOS users have great voice input tools like superwhisper and Aquavoice. I got tired of waiting for something similar on Linux, so I built koe. ホットキーを押して喋るだけ。Whisper で認識して、AI が文脈を見て整形してくれる。

**Reddit r/rust 用（技術寄り）**:
> koe: a voice input daemon for Linux written in Rust
>
> Uses whisper-rs, cpal for audio capture, rdev for global hotkeys, enigo for text input, x11rb for window context. Modular architecture - swap between local Whisper and OpenAI API, Claude and Ollama for post-processing.

---

## Channel Strategy

| チャネル | 施策 | リーチ | コスト | 優先度 | 対応 Issue |
|---|---|---|---|---|---|
| **README + デモ動画** | 30秒 GIF + 2分デモ動画を README に追加 | 全訪問者 | 時間のみ | **P0** | #30 |
| **HackerNews** | Show HN 投稿 | 大（フロントページで数千人） | 無料 | **P0** | #32 |
| **Reddit** | r/linux, r/rust, r/selfhosted に投稿 | 大（各 subreddit 数十万人） | 無料 | **P0** | #31 |
| **Awesome リスト** | awesome-rust, awesome-linux, awesome-whisper に PR | 中（SEO + 継続的流入） | 無料 | **P1** | #33 |
| **GitHub Topics** | リポジトリの topics を最適化 | 中（GitHub 検索経由） | 無料 | **P1** | — |
| **X (Twitter)** | ローンチスレッド + デモ動画 | 中 | 無料 | **P1** | — |
| **ブログ記事** | 技術的な設計判断を記事化（dev.to / Zenn） | 中（SEO 長期効果） | 時間のみ | **P2** | — |
| **Linux コミュニティ** | Discourse, フォーラム | 小〜中 | 無料 | **P2** | — |

---

## Launch Timeline

### Pre-launch（3/15 - 3/28）

| 日程 | アクション | 詳細 |
|---|---|---|
| 3/15-3/18 | **デモ素材制作** | 30秒 GIF（基本操作）+ 2分動画（セットアップから使用まで）。asciinema or OBS で録画 |
| 3/19-3/21 | **README 強化** | デモ GIF を冒頭に配置、「Why koe?」セクション追加、比較表（vs nerd-dictation）、ワンライナーインストール検討 |
| 3/22-3/24 | **GitHub リポジトリ整備** | CONTRIBUTING.md 作成、Issue テンプレート、GitHub Topics 最適化、v0.4.1 の Release を作成（バイナリ添付検討） |
| 3/25-3/27 | **投稿文の下書き** | HN, Reddit 各チャネル用の投稿文を準備。レビュー・推敲 |
| 3/28 | **最終チェック** | ビルド確認、README の全リンク確認、スクリーンショット最新化 |

### Launch Week（3/29 - 4/4）

| 日程 | アクション | 詳細 |
|---|---|---|
| 3/29（日） | **HackerNews 投稿** | Show HN 形式で投稿。太平洋時間の午前中（日本時間の深夜〜早朝）が最適。コメントに即レスできる時間帯を選ぶ |
| 3/30-3/31 | **HN 対応 + Reddit 投稿** | HN コメントに丁寧に返信。フィードバックを Issues に反映。HN の波が落ち着いたら Reddit 投稿（r/linux → r/rust → r/selfhosted の順） |
| 4/1-4/2 | **X スレッド + Awesome PR** | ローンチスレッド投稿。awesome-rust 等に PR を作成 |
| 4/3-4/4 | **フィードバック対応** | 報告されたバグの修正、機能リクエストの Issue 化、コントリビューター対応 |

### Post-launch（4/5 - 6/28、90日）

| 期間 | アクション |
|---|---|
| Week 2-3 | ローンチフィードバックを元に v0.5.0 リリース（クイックウィン機能追加 or バグ修正） |
| Week 4 | 技術ブログ記事公開（「Rust で Linux 音声入力を作った話」的な内容） |
| Week 5-8 | Wayland 対応、パッケージマネージャー配布（AUR, PPA 等）でインストール障壁を下げる |
| Week 9-12 | コミュニティ形成（Discord/Matrix? Discussions 活用?）、コントリビューター向けドキュメント充実 |

---

## Success Metrics

| 指標 | 30日目標 | 90日目標 |
|---|---|---|
| GitHub Stars | 100+ | 500+ |
| Forks | 10+ | 30+ |
| 外部コントリビューター（PR） | 3+ | 10+ |
| Open Issues（ユーザー起因） | 10+（関心の証拠） | — |
| Reddit/HN upvotes（合計） | 100+ | — |
| リリースバイナリ DL 数 | 50+ | 200+ |
| Awesome リスト掲載 | 1+ | 3+ |

---

## Risks & Mitigations

| リスク | 可能性 | 影響 | 対策 |
|---|---|---|---|
| X11 前提で Wayland ユーザーが使えない | 高 | 大 | README に X11 要件を明記。Wayland 対応をロードマップに入れて「対応予定」を示す |
| ビルドが難しくて離脱 | 高 | 大 | プリビルドバイナリの提供、AUR パッケージ作成、セットアップスクリプト |
| HN/Reddit で無視される | 中 | 中 | デモ動画の質が鍵。「superwhisper for Linux」のフレーミングで関心を引く |
| 「Whisper + AI って遅くない？」の指摘 | 中 | 小 | レイテンシ計測結果を用意。ローカル Whisper の速度を動画で見せる |
| Wayland 対応の要望殺到 | 高 | 中 | 事前に Issue を立てておき、ロードマップとして見せる。コントリビューション歓迎と明記 |
| API キー要件への批判（完全ローカルじゃない） | 中 | 小 | whisper-rs + Ollama の完全ローカル構成を README で強調 |

---

## Expansion Plan（ビーチヘッド後）

### Phase 2: Rust コミュニティへの浸透（90日後〜）
- crates.io への公開検討
- Rust 関連カンファレンス・ミートアップでの発表
- Rust 系ブログ・ポッドキャストへの露出

### Phase 3: セルフホスト / プライバシー層（6ヶ月後〜）
- 完全ローカル構成のワンコマンドセットアップ
- Docker イメージ提供
- r/selfhosted、r/privacy への定期的な更新投稿

### Phase 4: 多言語・非英語圏展開（9ヶ月後〜）
- 辞書機能の充実（コミュニティ辞書の共有基盤）
- 多言語 README（英語、日本語、中国語）
- 各言語コミュニティへの投稿

### Phase 5: アクセシビリティ（12ヶ月後〜）
- アクセシビリティ関連の Linux コミュニティとの連携
- 継続的音声入力モード（ディクテーション）
- AT-SPI2 連携
