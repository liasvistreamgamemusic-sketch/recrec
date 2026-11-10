# 事実抽出ガイド — fact sheet スキーマ・検出パターン・検証規則

Phase 2 のエージェント編成で使う。エージェントへのプロンプトには、このファイルの
該当セクション (Layer A なら「fact sheet スキーマ」、Layer B ①なら「環境変数の検出と確定」) を含めること。

## 共通ルール (全エージェント)

- 返すのは**事実**であって文章ではない。執筆は Phase 3 で行う — 抽出段階で要約・省略しない。
- 全ての事実に根拠 `file:line` を付ける。読まずに推測した事実は返してはならない。
  確認しきれなかったことは `"confidence": "unverified"` を付けて返す (黙って断定しない)。
- コード片を fact sheet にコピーしない (根拠は位置参照で足りる)。例外: エラーメッセージ文字列・
  コマンド文字列・設定キー名は値そのものが事実なのでそのまま記録する。
- secrets の実値は検出しても記録しない — キー名・場所・マスク済み先頭数文字のみ。
  tracked ファイル内の実値らしきものは `secrets_found` として即報告対象にする。

## Layer A: fact sheet スキーマ

チャンクごとに以下の JSON を返す。該当がないキーは空配列 (省略しない — 「無かった」ことも情報):

```json
{
  "chunk_id": "...",
  "files_reviewed": ["path", "..."],
  "features": [
    {
      "name": "機能名 (ドメイン語)",
      "purpose": "何のためか 1-2 文",
      "entrypoint": {"kind": "api|cli|ui|schedule|event|hook", "detail": "POST /api/x 等"},
      "inputs": "受け取るもの・バリデーション条件",
      "flow": ["段階1", "段階2 (分岐条件含む)"],
      "outputs": "返すもの・保存するもの・通知",
      "error_behavior": [{"case": "失敗パターン", "behavior": "何が起きるか"}],
      "related_config": ["ENV_VAR名", "設定キー"],
      "evidence": ["file:line"]
    }
  ],
  "env_vars": [
    {
      "name": "VAR_NAME",
      "read_at": ["file:line"],
      "fallback": "コード上のフォールバック値 or null",
      "fails_without": true,
      "values_seen": ["コード内で比較・分岐に使われている値"],
      "is_secret": false
    }
  ],
  "config_keys": [{"file": "path", "key": "...", "meaning": "...", "evidence": "file:line"}],
  "interfaces": [{"kind": "api|cli|route|webhook|event", "signature": "GET /x, cmd --flag 等",
                  "auth": "要/不要/不明", "summary": "...", "evidence": "file:line"}],
  "data_models": [{"name": "...", "storage": "table/collection/file名",
                   "fields": [{"name": "...", "meaning": "...", "constraints": "enum値・NOT NULL 等"}],
                   "relations": ["他モデルとの関係"], "evidence": "file:line"}],
  "jobs": [{"name": "...", "trigger": "cron式/キュー名/イベント", "summary": "...",
            "on_failure": "リトライ・通知等", "evidence": "file:line"}],
  "external_deps": [{"service": "...", "purpose": "...", "config_via": ["ENV_VAR"], "evidence": "file:line"}],
  "error_catalog": [{"message": "エラーメッセージ文字列そのまま", "condition": "発生条件",
                     "evidence": "file:line"}],
  "commands": [{"cmd": "...", "purpose": "...", "defined_in": "package.json scripts / Makefile 等"}],
  "glossary_candidates": [{"term": "...", "meaning": "...", "identifier": "コード内の名前"}],
  "secrets_found": [{"file": "path", "key_name": "...", "masked": "sk-ab****"}],
  "doc_contradictions": [{"doc": "README:12", "doc_says": "...", "code_says": "...", "evidence": "file:line"}]
}
```

`error_catalog` は 11 章 (トラブルシューティング) の一次材料 — ユーザー入力・設定・外部要因で
発生しうるものを優先して拾う (プログラミングエラー由来の assert 等は省いてよい)。

## Layer B ①: 環境変数の検出と確定

### 検出 (grep — Layer A の申告と独立に行い、和集合を取る)

エコシステムに応じて以下を全体 grep する。ヒットした変数名を正規化して一覧化する:

- **Node/TS**: `process.env.`, `process.env[`, `import.meta.env`, `Deno.env.get`
- **Python**: `os.environ`, `os.getenv`, `environ.get`, pydantic `BaseSettings` / `Field(env=`, `dotenv`
- **Go**: `os.Getenv`, `os.LookupEnv`, `viper.`, `envconfig`, struct タグ `env:"`
- **Ruby**: `ENV[`, `ENV.fetch`
- **Java/Kotlin**: `System.getenv`, `@Value("${`, `application.yml` の `${VAR}` 参照
- **Rust**: `std::env::var`, `env!(`, `dotenvy`
- **PHP**: `getenv(`, `$_ENV`, `env(` (Laravel)
- **シェル/CI/インフラ**: `${VAR}` / `$VAR` (Dockerfile `ENV`/`ARG`, docker-compose `environment`,
  k8s manifest, Terraform, GitHub Actions / GitLab CI の `variables:`)
- **宣言ファイル**: `.env.example`, `.env.sample`, `app.json`, helm values 等

フレームワーク規約の暗黙変数も含める (例: `DATABASE_URL`, `PORT`, `NODE_ENV`, `RAILS_ENV`,
`DJANGO_SETTINGS_MODULE`) — コード上に読み取りが見えなくてもフレームワークが読むものは表に載せる
(説明に「フレームワークが直接参照」と書く)。

### 確定 (変数ごとに読み取り箇所を精読して埋める)

| 項目 | 決め方 |
| --- | --- |
| 必須性 | 未設定時のコードパスを追う: 例外/exit → `必須`。フォールバックで動く → `任意`。特定機能の分岐内のみ → `条件付き必須 (○○利用時)` |
| デフォルト | フォールバック式の値そのまま (`os.getenv("X", "10")` → `10`)。多段フォールバックは優先順も記録 |
| 取りうる値 | その変数が比較される全箇所を追う: `== "production"` 等の比較値・enum・バリデーション (zod/pydantic 等) の定義から列挙。数値は単位を使用箇所から特定 (ms か秒か)。真偽値はコードの真偽判定方法 (`=== "true"` か truthy か) |
| 変更時の影響 | 読み取りタイミング (起動時 1 回 or 毎回) から再起動要否を判定。影響範囲は使用箇所の機能名で書く |
| secret 判定 | 名前 (`KEY|TOKEN|SECRET|PASSWORD|CREDENTIAL`) と用途から。表では値例を書かない |

### 整合表 (4.3 章の材料)

検出した全変数 × 環境定義ファイル (`.env.example` / compose / CI / IaC / k8s) の掲載有無マトリクス。
コードが読むのにどこにも宣言されていない変数、宣言されているのにコードが読まない変数 (死んだ設定候補)
の両方を欠落として記録する。

## Layer B ②〜⑤: 横断観点

- **② アーキテクチャ**: エントリポイント (main / server 起動 / handler 登録) から依存方向に追い、
  コンポーネント・通信・データストアを図の材料として構造化する。ディレクトリ構成の役割も 1 行ずつ。
- **③ サーフェス集約**: 全チャンクの `interfaces` を集約・重複排除し、ルーター定義と突合して
  漏れを grep で裏取りする (ルート登録のパターン: `router.`, `@app.route`, `urls.py`, controller アノテーション等)。
- **④ データモデル集約**: `data_models` を集約し、スキーマ定義 (migrations / schema.prisma /
  models / DDL) と突合。ER の関係線と enum カラムの全値を確定する。
- **⑤ セットアップ動線**: README・Dockerfile・compose・CI・manifest (engines 等) を突合し、
  前提バージョン・インストール・初期化・起動・動作確認の一本道を組む。README の手順は
  「コード/設定に裏付けがあるか」を検証してから採用する (裏付けなし → ⚠️ 未確認)。

## Layer C: 検証規則

対象は説明書の信頼を左右する load-bearing な事実。1 件ずつ反証を試みる:

- **env var のデフォルト/必須性**: レジストリの記述と読み取りコードを再照合。複数箇所で読んでいて
  フォールバックが食い違う場合は「箇所により異なる」と正直に書く材料にする。
- **コマンド**: 定義元 (package.json / Makefile / justfile / CI) に文字通り存在するか。
  README にだけ書かれているコマンドは未確認扱い。
- **API パス**: ルーター定義の prefix (`/api/v1` 等のマウント) を合成した最終パスになっているか。
- **バージョン前提**: manifest / lockfile / Dockerfile ベースイメージ / CI の指定と一致するか。
- 反証が成立したら Layer B / fact sheet を修正し、確定できなければ `unverified` に落とす。

## 差分更新モード

前回ワークスペースの `evidence.json` と fact sheet を読み込み:

1. `git diff --name-only <前回コミット>..HEAD` の変更ファイルが属するチャンクのみ Layer A を再実行
2. 環境変数レジストリ・サーフェス集約 (Layer B ①③) は grep が安いので**毎回全体を再実行**する
3. 変更された事実が根拠になっている章を `evidence.json` から逆引きして、その章だけ書き直す
4. 0 章 (対象コミット・カバレッジ) と 13 章 (付録) は必ず再生成する
