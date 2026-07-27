<!-- このファイルはプロジェクト固有ルールのみを書く。個人/グローバル AI ルール
（言語・確認スタイル・出力フォーマット等）は各 AI ツールのグローバル設定へ。
fresh public clone でも有効な内容に保つこと。 -->

# audioremote 開発ガイド

## プロジェクト概要

Windows 11 ホスト（ホスト）の**既定音声出力デバイス**を、同じ LAN 上の任意のブラウザ／クライアントから切り替える軽量ローカルサービス。ホストの前に戻らず、Hyper-V ゲスト・WSL2・スマホ・別 PC から出力先（例: Nest Hub Max ⇄ 有線イヤホン ⇄ ヘッドフォン）を変えられるようにする。

構成は「ホスト常駐 Rust サーバー ＋ HTTP API ＋ 薄いクライアント」。切替は Windows Core Audio の `IMMDeviceEnumerator` で列挙し、非公開 COM `IPolicyConfig::SetDefaultEndpoint` で Console/Multimedia/Communications の 3 役割をまとめて変更する（会議アプリの Communications 既定漏れを防ぐため常に 3 役割一括）。表示名: `AudioRemote`。

## やらないこと（スコープ外）

- macOS / Linux サーバー実装（Windows Core Audio 前提）
- Windows 10 の正式対応（v0.1 は Windows 11 のみ）
- コード署名 / winget 登録（Node 保有ユーザー向け npm 配布で SmartScreen を回避する方針。要件が出るまで deferred）
- Console / Multimedia / Communications の**個別切替 UI**（常に 3 役割まとめて切り替える）
- 自己署名 / mkcert 等の本格 HTTPS（v0.1 は HTTP 既定 + 任意有効化のみ）
- ロードマップ範囲外の入口（PWA / アプリ窓等）は v0.3 以降で判断

## 技術スタック

| レイヤ | 採用 |
|---|---|
| 言語 | Rust (edition 2021, バイナリ crate) |
| ビルド | Cargo |
| 対象 OS（サーバー） | Windows 11 のみ |
| Windows API | `windows` crate（COM）／`IMMDeviceEnumerator` ／ 非公開 `IPolicyConfig` |
| HTTP サーバー | axum 等の軽量 crate（予定） |
| 静的アセット同梱 | `rust-embed` 等で単一バイナリに Web UI を埋め込み（予定） |
| 設定 | `%APPDATA%\audioremote\config.toml`（bind / require_token / allowed_networks） |
| 認証 | `Authorization: Bearer <token>`（初回生成・名前付き複数トークン・失効可。`audioremote token add\|revoke\|list`） |
| 配布（主） | npm レジストリ（Rust exe を optionalDependencies でプラットフォーム別に同梱） |
| 配布（副） | GitHub Releases（exe + SHA256SUMS・未署名） |

## ディレクトリ構成

- `src/main.rs` — バイナリ crate エントリ（現状は crates.io 予約用 Hello world スタブ）
- `Cargo.toml` — パッケージ定義（`name = "audioremote"` / `version = "0.0.0"` 予約占有中・本リリースは `0.1.0` から）
- `docs/local/` — plan / recap / bugfix / pending（**gitignore・非公開**。"local" の名のとおり追跡しない。公開したい開発ドキュメントは `docs/` 直下へ置く）
  - `plan_audioremote-v0.1.md` — v0.1 の作業分割（C1〜C5）と API 仕様の正本
  - `recap_2026-07-24_audioremote-naming-reserve.md` — 命名確定・3 チャネル名前予約・クライアント形決定
- `scripts/` — `secrets-scan.mjs` / `install-hooks.{sh,ps1}`
- `.githooks/` — layer 2 pre-commit（`core.hooksPath = .githooks` で有効化）
- `.github/workflows/` — layer 3 CI (`secrets-scan.yml`)

## 開発時のデプロイ構成（重要）

- **開発機（ゲスト）**: この repo で `cargo build --release` するマシン。音声デバイスは持たない or 使わない
- **稼働機（ホスト）**: 実際に audioremote を走らせる Windows 11。物理コンソールセッション必須（RDP 不可）。Nest Hub Max / 有線イヤホン等の実デバイスが繋がっている
- **配線**: 両者は同 LAN 上・物理隣接。exe は `target\release\audioremote.exe` を手コピーでホストへ移送し、ホスト側でダブルクリック起動する
- **検証**: ゲストのブラウザから `http://<host-lan-ip>:17650/` にアクセスして API と Web UI を検証（loopback から見えるのはゲスト自身の `リモート オーディオ` だけなので実 UX にならない）

## 主要コマンド

- ビルド: `cargo build`
- 実行: `cargo run`
- テスト: `cargo test`
- lint: `cargo clippy`
- 整形: `cargo fmt`
- 自動起動登録: `audioremote --install-autostart`
- 自動起動解除: `audioremote --uninstall-autostart`
- secrets-scan 手動実行: `node scripts/secrets-scan.mjs --staged --block`

## AI 作業共通ルール

ビルド・コミット禁止、secrets-scan 責務、plan/bugfix/pending md の作成ルール等の AI 作業共通ルールは、各利用者のグローバル AI 設定に従う（作者環境の例: `~/.claude/CLAUDE.md` および `~/.claude/guides/`）。

このリポジトリ固有:

- **v0.1 の作業単位は `docs/local/plan_audioremote-v0.1.md` の C1〜C5 に従う**（実行順序: C1 → C2 → (C3, C4) → C5）
- **音声デバイス切替は必ず Windows のデバイス ID で行う**（表示名は再接続等で変わりうる）
- **切替 API は 3 役割（Console/Multimedia/Communications）まとめて変更する**（個別切替は v0.1 スコープ外）
- **サーバーはログオンユーザーのセッションで動かす**（SYSTEM サービスにしない。音声デバイスは対話ユーザー所属のため）
- **HTTP bind の既定は `0.0.0.0`（LAN 公開・2026-07-24 LAN-first 転換）**。非 loopback クライアントは Bearer トークン必須・loopback（ホスト自身）は素通し。`127.0.0.1` に閉じたい場合は `audioremote setup`。Host ヘッダ許可リストで DNS rebinding を防ぎ、`allowed_networks`（CIDR）で送信元 IP を絞れる
- **`crates.io` 版の `0.0.x` は予約占有用**。本リリースは `0.1.0` から（yank しても名前は永久占有される点に注意）

## secrets-scan（このリポジトリの配線）

書く瞬間の責務（固有名詞の一般化・fixture は合成データ等）は上記「AI 作業共通ルール」の参照先に従う。このリポジトリ固有の配線は以下:

- scanner: `scripts/secrets-scan.mjs`（手動実行: `node scripts/secrets-scan.mjs --staged --block`）
- layer 2: `.githooks/pre-commit`（`core.hooksPath = .githooks` で有効化。第三者 clone 時は `bash scripts/install-hooks.sh` または `pwsh scripts/install-hooks.ps1`）
- layer 3: `.github/workflows/secrets-scan.yml`（server-side backstop）
- layer 4: release skill 側で kb 全文突き合わせ
- env（full coverage に必要・未設定なら構造 regex のみで継続）: `KB_ROOT` / `FAMILY_ROOT`。設定詳細は `scripts/secrets-scan.mjs` の冒頭コメント
- 参照実装・設計詳細: `worklog-bridge` リポの `docs/local/secrets-scan-design/`（gitignored・公開しない）

## 関連ドキュメント

| 項目 | パス |
|---|---|
| ユーザー向け README | `README.md` |
| Codex/他 AI 用入口 | `AGENTS.md` |
| v0.1 実装計画（正本） | `docs/local/plan_audioremote-v0.1.md` |
| 命名・配布方針の経緯 | `docs/local/recap_2026-07-24_audioremote-naming-reserve.md` |
