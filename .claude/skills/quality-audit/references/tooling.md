# 静的ツールマトリクス — Phase 3.0 ツール事前スキャン

機械が確実に検出できるもの (未使用 import・既知 CVE・secrets パターン・型エラー) は、
エージェントの精読より先にツールで洗い出す。エージェントの仕事を「発見」から「検証と文脈判断」に
変えることが、このスキルの速度と精度の両方を支える。

## 原則

- **プロジェクトにインストール済みのツール・設定が常に最優先。** lint 設定はプロジェクトの「正」であり、
  勝手に別ツールや別ルールセットで裁かない。
- 未インストールでも、著名なツールは一時実行 (`npx`, `pnpm dlx`, `uvx`, `pipx run`, `go run`) してよい。
  ただし **プロジェクトの依存ファイル (package.json / pyproject.toml / requirements / Cargo.toml) に追加しない**。
  グローバルインストールもしない。ネットワークが使えない環境では下部の grep フォールバックを使う。
- ツール出力は**偽陽性を含む前提**で扱う。finding の根拠の一つにはなるが、単独で自動修正の根拠にしない。
  「ツールと精読の両方が指摘 + Layer C で反証なし」が削除・修正の条件 (audit-dimensions.md 参照)。
- 各ツールの生出力はワークスペース `tools/<tool名>.txt` に保存し、チャンク別に該当行を抽出して
  Layer A エージェントのプロンプトに「ツール所見」として添付する。
- ツールの実行失敗 (未対応バージョン・設定不備) は握りつぶさず「実行できなかったツールと理由」として
  レポートのカバレッジ欄に記載する — 「スキャン済み」と「スキャンできず」の区別は保証の一部。

## 言語横断 (全プロジェクトで実施)

| 目的 | 第一選択 | コマンド例 | 備考 |
|---|---|---|---|
| secrets 検出 | gitleaks | `gitleaks detect --no-banner -v` | `--log-opts="--all"` で git 履歴も対象化 |
| secrets (代替) | trufflehog / grep | 下記フォールバックパターン | |
| 依存の既知脆弱性 | osv-scanner | `osv-scanner scan -r .` | 無ければ各エコシステムの audit (下記) |
| 巨大ファイル検出 | wc | `git ls-files | xargs wc -l | sort -rn | head -30` | structure 次元の候補出し |

## JavaScript / TypeScript

| 目的 | ツール | コマンド例 |
|---|---|---|
| lint | プロジェクトの eslint / biome | `npx eslint . --format json` / `npx biome check .` |
| 型 | tsc | `npx tsc --noEmit` |
| デッドコード + 未使用依存 | knip (第一選択) | `npx knip --reporter json` |
| デッドコード (代替) | ts-prune / depcheck | `npx ts-prune` / `npx depcheck --json` |
| 依存脆弱性 | npm audit | `npm audit --json` (pnpm/yarn は各 audit) |

knip の注意: 動的 import・設定ファイル参照 (next.config 等) を見逃すことがある。エントリポイント設定が
ないプロジェクトでは偽陽性が増えるため、Layer C 検証を必ず通す。

## Python

| 目的 | ツール | コマンド例 |
|---|---|---|
| lint + 未使用 | ruff | `uvx ruff check --output-format json .` (F401 未使用 import, F841 未使用変数) |
| 型 | mypy / pyright | プロジェクトが型チェックを運用している場合のみ、その設定で実行 |
| デッドコード | vulture | `uvx vulture . --min-confidence 80` (偽陽性多め — Layer C 必須) |
| 依存脆弱性 | pip-audit | `uvx pip-audit` |

## その他のエコシステム

- **Go**: `go vet ./...`, `staticcheck ./...`, `govulncheck ./...`
- **Rust**: `cargo clippy --all-targets`, `cargo audit`, `cargo +nightly udeps` (未使用依存)
- **Ruby**: `rubocop --format json`, `bundler-audit check`
- **PHP**: `phpstan analyse`, `composer audit`
- **Terraform / IaC**: `tflint`, `tfsec` または `trivy config .` (公開 S3・過剰 IAM 等の設定ミス検出)
- **Docker / CI**: `hadolint Dockerfile`, `trivy config .`

該当エコシステムのツールがこの表になければ、そのエコシステムの標準的な linter / audit ツールを
同じ原則 (インストール済み優先・依存に追加しない・偽陽性前提) で使う。

## secrets の grep フォールバックパターン

gitleaks / trufflehog が使えない場合の最低限。ヒットは全て偽陽性前提で Layer C 検証に回す:

```
AKIA[0-9A-Z]{16}                                      # AWS アクセスキー
-----BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY-----        # 秘密鍵
(api[_-]?key|secret|token|passw(or)?d)\s*[:=]\s*['"][^'"]{8,}['"]   # 汎用代入
ghp_[A-Za-z0-9]{36}|glpat-[A-Za-z0-9_-]{20,}           # GitHub / GitLab トークン
sk-[A-Za-z0-9_-]{20,}                                  # OpenAI/Anthropic 系 API キー
```

**重要: ヒットした値そのものを findings・レポート・プロンプトに転記しない。**
場所 (file:line)・キー名・先頭数文字 + マスク (`glpat-b5**********`) のみ記録する (SKILL.md 絶対原則 5)。
`.env` など untracked ファイル内の secrets は「配置は正しい」ので flag しない — tracked ファイルと
git 履歴内のものだけが問題。
