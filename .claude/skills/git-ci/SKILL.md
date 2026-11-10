---
name: git-ci
description: >-
  CI / パイプラインの状態を確認し、失敗に対応するスキル。ユーザーが「CI 見て」「パイプライン確認して」
  「テスト通った?」「CI 落ちてる」等と言ったら、またはスラッシュコマンドで呼ばれたら必ず使用。
  まず状態を即座に報告し、失敗があるときだけ対応方針をボタン質問で確認する。
  GitHub Actions (gh) / GitLab CI (glab) を remote から自動判定する。
---

# git-ci — CI 確認と失敗対応

## 手順

### 1. 状態確認 (質問なしで即実行・即報告)

```bash
git remote get-url origin          # github.com → gh / それ以外の GitLab → glab
# GitHub:
gh pr checks 2>/dev/null || gh run list --limit 5
# GitLab:
glab ci status                     # 現在ブランチのパイプライン
```

- **全部成功 → そのまま日本語で報告して終了 (質問しない)**
- 実行中 → 状態を報告し、`gh run watch` / `glab ci status` での追跡が必要か文脈で判断

### 2. 失敗があるときだけ AskUserQuestion

まず失敗ジョブのログを取得して**原因の見立て**を作る:

```bash
gh run view <run-id> --log-failed          # GitHub: 失敗ステップのログ
glab ci trace <ジョブ名>                    # GitLab: ジョブログ
```

**失敗ジョブ・失敗内容・原因の見立ては質問の前に本文で報告する** —
選択肢の説明欄だけに載せない (内容が見えないまま対応方針を選ばせない)。

| # | 質問 | 選択肢 (先頭=推奨) | 備考 |
| --- | --- | --- | --- |
| 1 | 対応 | 「原因を調査して修正まで (推奨)」 / 「失敗ログの要約だけ」 / 「リトライ (flaky 疑い)」 | 説明に原因の見立てを 1 行で載せる |

- リトライ: `gh run rerun <run-id> --failed` / `glab ci retry <job-id>`。ただし**根本原因を隠すためのリトライ連打はしない** (2 回失敗したら調査に切り替え)

### 3. 修正する場合

- **`superpowers:systematic-debugging` で直す** (ログ全文を読む → ローカル再現 → 根本原因の特定 → 回帰テスト付きで修正) → 再 push で CI 再実行。失敗ジョブのログ全文と再現コマンドを調査の入力にする
- `.gitlab-ci.yml` / workflow YAML を変更したときは push 前に構文チェック (`glab ci lint` / actionlint があれば)

### 4. 報告

- 例: 「CI 失敗: `test` ジョブで `test_rounding` が落ちています。原因は丸め誤差 (見立て) — 修正して push 済み、再実行中」
- 成功: 「CI 全て成功しています (5 ジョブ・3分12秒)」のように具体的に。数字は実際の出力から
