# recrec 実装計画

設計: `docs/superpowers/specs/2026-11-10-recrec-design.md`

各ステップは「テストを書く → 実装 → `cargo test` が通る」で閉じる。UI 以外は Linux でテストできる。

1. Cargo 構成 (`asio` feature、Windows 限定依存、release プロファイル)、`lib.rs` の骨組み
2. `dither.rs` / `wav.rs` (書き出し・ヘッダ更新・RF64・BWF・修復) とテスト
3. `meter.rs` / `audio/capture.rs` (形式変換・フレーム単位のリング書き込み) とテスト
4. `track.rs` / `take.rs` (命名・サニタイズ・メタデータ・マーカー・修復走査・一覧) とテスト
5. `recorder.rs` (書き込みスレッド・プリロール・ヘッダ更新・残量監視) と合成信号の結合テスト
6. `config.rs` とテスト
7. `audio/devices.rs` / `audio/stream.rs` (cpal)、`platform.rs`
8. UI (`ui/app.rs` ほか)、`main.rs`
9. Windows (gnu ツールチェーン) でビルドし、WASAPI で起動・録音のスモークテスト
10. CI / リリースワークフロー、Inno Setup スクリプト、README・LICENSE
11. ブランチ → PR → CI グリーン → マージ → `v0.1.0` タグ → Release の成果物を確認
