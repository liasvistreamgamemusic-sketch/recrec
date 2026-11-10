# recrec

オーディオインターフェースの全入力を、**入力ごとに別々の WAV ファイル**で同時に録る、
ミュージシャン向けの録音専用アプリです。Windows 用・無料・広告なし・オープンソース (GPL-3.0)。

- **ASIO / WASAPI 対応** — オーディオインターフェースを ASIO で直接録れます
- **入力ごとにモノラル WAV** — In1 のマイクと In2 のギターが、頭のそろった別ファイルになります。
  In3+4 をステレオ 1 ファイルにする、といった組み合わせも自由
- **16bit / 24bit / 32bit float** — 24bit 入力はビット単位で無劣化。16bit は TPDF ディザ付き
- **プリロール** — 録音ボタンを押す前の最大 30 秒もさかのぼって録れます
- **落ちても録音が残る** — ヘッダを 1 秒ごとに更新し、異常終了したテイクは次の起動時に自動修復
- **長時間録音** — 4GB を超えると自動で RF64 に切り替え。録音中はスリープしません
- **DAW へドラッグ** — 録ったファイルをウィンドウから DAW へそのままドロップ
- **BWF タイムスタンプ** — 録音開始時刻をファイルに記録 (対応 DAW では元の時間位置に配置できます)

## インストール

[Releases](https://github.com/liasvistreamgamemusic-sketch/recrec/releases) から次のどちらかをダウンロードします。

| ファイル | 内容 |
|---|---|
| `recrec-<版>-setup.exe` | インストーラー。管理者権限は不要 (ユーザー単位でインストール)。スタートメニューに登録され、アンインストールもできます |
| `recrec-<版>-windows-x64.zip` | ポータブル版。展開して `recrec.exe` を実行するだけ |

コード署名をしていないため、初回は「Windows によって PC が保護されました」と表示されることがあります。
「詳細情報」→「実行」で起動できます。ダウンロードしたファイルは `SHA256SUMS.txt` で照合できます。

## 使い方

1. 上の欄で **ホスト** (`ASIO` 推奨)・**デバイス**・**レート**・**バッファ** を選びます
2. 「トラック」に、録音するファイルの一覧が出ます。既定は「入力 1 つにつき 1 トラック (モノラル)」です
   - ● … 録音する / しない
   - 名前 … ファイル名になります (例: `Vocal` → `01_Vocal.wav`)
   - ソース … `In 1` のようなモノラル、または `In 3+4 (ステレオ)`
3. 大きな ● ボタン (または **Space** キー) で録音開始、もう一度押すと停止
4. 「テイク」の `☰` をつかんで DAW へドラッグします (テイク行の `☰` は全ファイル、ファイル名の `☰` は 1 ファイル)

### 4 入力のオーディオインターフェースの例

| 入力 | つなぐもの | トラック設定 |
|---|---|---|
| In 1 | ボーカルマイク | `Vocal` / In 1 |
| In 2 | ギター (Hi-Z) | `Guitar` / In 2 |
| In 3-4 | シンセ (ステレオ) | `Synth` / In 3+4 (ステレオ) |

モニターはオーディオインターフェース本体のダイレクトモニター機能を使ってください。recrec は録音に専念し、音は出しません。

### 保存先

既定はミュージックフォルダの `recrec` です (「出力先」の「変更…」で変えられます)。

```text
recrec/
  2026-11-10_223015/          ← テイク (録音開始時刻)
    01_Vocal.wav
    02_Guitar.wav
    03_Synth.wav
    take.json                 ← デバイス・形式・長さ・取りこぼし数などの記録
  2026-11-10_224102_サビ案/    ← テイク名を入れた場合
```

### 困ったとき

| 症状 | 対処 |
|---|---|
| ASIO でデバイスを開けない | 他のアプリ (DAW など) が ASIO ドライバを使っていないか確認し、「⟳ 再接続」 |
| 状態欄に「取りこぼし」が出る | バッファを大きくする。録音中は重い処理を避ける |
| 録音中に PC が落ちた | もう一度起動すると自動で修復されます (失うのは最後の約 1 秒) |
| ログを見たい | `%LOCALAPPDATA%\recrec\data\logs\recrec.log` |

設定は `%APPDATA%\recrec\config\config.json` に保存されます。

## ソースからビルド

Rust (stable) が必要です。

```sh
cargo test
cargo run --release                      # WASAPI のみ
cargo build --release --features asio    # ASIO 対応 (Windows)
```

ASIO 対応ビルドには LLVM (libclang) が必要です (`LIBCLANG_PATH` を LLVM の `bin` に設定)。
ASIO SDK はビルド時に Steinberg のサイトから自動で取得されます。

設計は [docs/superpowers/specs/2026-11-10-recrec-design.md](docs/superpowers/specs/2026-11-10-recrec-design.md) にあります。

## ライセンス

GPL-3.0-or-later ([LICENSE](LICENSE))。
Steinberg ASIO SDK を GPLv3 のライセンスで利用しています。
ASIO is a trademark and software of Steinberg Media Technologies GmbH.
