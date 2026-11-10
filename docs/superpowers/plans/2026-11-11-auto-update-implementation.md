# 起動時のアップデート確認とワンクリック更新 — 実装計画

設計: `docs/superpowers/specs/2026-11-11-auto-update-design.md`

各ステップは「テストを書く → 実装 → `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` /
`cargo test` が通る」で閉じ、ステップごとにコミットする。ステップ 1〜5 は Linux でテストできる。

## 前提

- ブランチ: `feat/auto-update`。バナーは新 UI (`feat/ui-redesign`) の上に載るので、
  `feat/ui-redesign` が `main` に入っていればそこから、未マージならその先端から切る
- 追加クレート (2026-11-11 に `cargo search` で確認した最新版): `ureq` 3.5.1 / `sha2` 0.11.1 / `semver` 1.0.28。
  **API は未確認** — ステップ 1 と 4 で docs.rs を見て確定する (特に `ureq` 3 系のタイムアウト設定・
  ストリーム読み出しと、`sha2` 0.11 のハッシュ出力の 16 進化)

## ステップ

1. **依存の追加** — `cargo add ureq sha2 semver`。`ureq` は使う機能だけに絞る (rustls、JSON 機能は不要 —
   解釈は既存の `serde_json`)。各クレートのライセンスが GPL-3.0-or-later と両立することを確認する。
   `Cargo.lock` を更新し、Linux と Windows (CI) の両方でビルドが通ること

2. **`config.rs`** — `AppConfig` に `skip_update_check: bool` と `skipped_version: Option<String>` を追加。
   テスト: 既存の `save_then_load_roundtrips` に新キーを含める。新キーの無いファイルが既定値
   (false / None) で読めること (`unknown_and_missing_fields_are_tolerated` を拡張)

3. **`update.rs` の純粋部分** (新規、`lib.rs` に `pub mod update;`) — ネットワークに触らない関数とテスト
   - `Release { version: semver::Version, page_url, installer: Option<Asset>, checksums: Option<Asset> }`
   - `parse_release(json: &[u8]) -> Result<Release, UpdateError>` — `tag_name` の `v` を外して SemVer 化、
     アセットを `recrec-<ver>-setup.exe` / `SHA256SUMS.txt` の名前で選ぶ。URL が `https://github.com/`
     で始まらないアセットは拒否
   - `is_newer(release, current, skipped) -> bool` — 厳密に新しく、スキップ済みでないときだけ true
   - `parse_checksum(sums: &str, file_name: &str) -> Option<String>` — 大文字小文字・CRLF を許容
   - `releases_api_url()` — `env!("CARGO_PKG_REPOSITORY")` から組み立てる
   - エラー型は既存に合わせて `thiserror`
   - テスト項目は設計書 §7 のとおり

4. **`update.rs` の通信部分**
   - `check_latest() -> Result<Release, UpdateError>` — タイムアウト 10 秒、User-Agent `recrec/<ver>`
   - `download_installer(release, dir, on_progress) -> Result<PathBuf, UpdateError>` — `.part` に書き、
     上限 200 MB、SHA256 を照合してからリネーム。失敗時は `.part` を削除
   - `clean_update_dir(dir)` — 起動時に古いファイルを消す
   - `config::update_dir()` を `log_dir()` と同じ形で追加
   - 照合と後始末は、読み出し元を `impl Read` で受ける内部関数に分け、`tempfile` で
     「一致でファイルが残る / 不一致・サイズ超過でファイルが消える」をテストする。HTTP 自体はテストしない

5. **`platform.rs`**
   - `open_url(url) -> io::Result<()>` — 既存の `open_in_file_manager` と同じプログラム選択を共有する
     (重複させず内部関数に寄せる)。呼び出し側は `https://github.com/` で始まる検証済み URL だけを渡す
   - `is_installed_build() -> bool` — 実行中の exe と同じフォルダーに `unins000.exe` があるか。
     Windows 以外は常に false
   - `launch_installer(path) -> io::Result<()>` — `/SILENT` を付けて起動 (Windows のみ)

6. **`ui/app.rs`** — 状態とスレッド
   - `UpdateState { Idle, Available(Release), Downloading { release, received, total }, Ready { release, path }, Dismissed }`
     と `update_rx: Option<Receiver<UpdateEvent>>` を `RecrecApp` に追加
   - `launch()`: `clean_update_dir`、`skip_update_check` が false なら確認スレッドを起動
   - `poll()`: イベントを受けて状態を進める。確認失敗は `log::warn!` のみ。`Ready` かつ `!is_busy()` なら
     `launch_installer` → `shutdown()` → ビューポートを閉じる。失敗は `push(Level::Error, …)`
   - アクション: `start_update` / `open_release_page` / `skip_update` (設定に保存) / `dismiss_update` /
     `set_update_check`
   - チャネル切断 (スレッドの panic) は更新機能だけ止め、ログに残す

7. **`ui/view.rs` / `ui/widgets.rs`** — バナーと設定
   - 上部バナー: `is_busy()` の間は描かない。文言・ボタンは設計書 §2。ダウンロード中は既存の
     `usage_bar` で進捗を出す。色・余白・アイコンは `theme.rs` の既存トークンと Phosphor アイコンを使う
   - `Ready` で録音中なら「録音を止めると更新できます」
   - 設定パネル (プリロールの並び) に「起動時にアップデートを確認する」
   - Linux でウィンドウを起動し、テスト用に差し替えた応答でバナーの各状態を目視確認する

8. **インストーラーとリリース**
   - `installer/recrec.iss`: `[Run]` の `skipifsilent` を外す
   - `release.yml`: アセット名がアプリとの契約であることをコメントに明記
   - `installer/release-notes.md`: 0.1.0 からは手動更新が必要なこと、以後は自動で通知されること
   - `README.md`: 起動時に GitHub へ最新版を問い合わせること、設定でオフにできること
   - `Cargo.toml` のバージョンを 0.2.0 に上げる

9. **Windows 実機確認** (設計書 §7。ここが通るまでリリースしない)
   - テスト用に版を下げたビルドをインストーラーで入れ、実リリースに対して:
     バナー表示 → 更新 → `/SILENT` インストール → **自動で開き直るか** (設計書 §4.4 の未確認点)
   - 管理者インストール (全ユーザー向け) でも同じ流れが通るか
   - スキップ、×、設定オフ、オフライン起動、録音中の抑止、ダウンロード中に録音開始、ポータブル版
   - 開き直らない場合は、更新専用の `[Run]` エントリ方式に切り替えて再確認する

10. **PR → CI グリーン → マージ → `v0.2.0` タグ → Release の成果物確認**
