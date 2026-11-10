---
name: git-merge
description: >-
  PR / MR をマージするスキル。ユーザーが「マージして」「取り込んで」「merge」等と言ったら、
  またはスラッシュコマンドで呼ばれたら必ず使用。対象・マージ方式 (squash 等)・ブランチ削除を
  ボタン質問 1 回で確認してから実行する (この質問がマージの最終承認を兼ねる)。
  CI が失敗している場合はマージせず先に報告する。GitHub (gh) / GitLab (glab) を自動判定。
---

# git-merge — ボタンで完結するマージ (質問=最終承認)

## 手順

### 0. ホスト判定と対象一覧

```bash
git remote get-url origin          # github.com → gh / それ以外の GitLab → glab
gh pr list --limit 4               # または glab mr list
```

### 1. マージ可否チェック (質問の前に必ず)

```bash
gh pr checks <番号>                # GitHub: CI 状態
glab ci status                     # GitLab: パイプライン状態
gh pr view <番号>                  # 承認状況・コンフリクト有無
```

- **CI が失敗している / コンフリクトがある → 質問せず停止して報告** (直すかどうかは別の判断)
- CI 実行中なら「完了を待ってからマージするか」を質問の選択肢に含める

### 2. AskUserQuestion で 1 回だけ質問 (= 最終承認)

| # | 質問 | 選択肢 (先頭=推奨) | 備考 |
| --- | --- | --- | --- |
| 1 | 対象 | 現在ブランチの PR/MR を先頭に「(推奨)」+ 他のオープン分 (最大4) | 番号+タイトル。説明に CI/承認状態を明記 |
| 2 | 方式 | 「squash (推奨)」 / 「merge commit」 / 「rebase」 | プロジェクトで無効な方式は選択肢から外す。慣例が分かればそれを推奨に |
| 3 | ブランチ削除 | 「する (推奨)」 / 「しない」 | |

方式の意味は説明欄に一言添える (squash = コミットを 1 つにまとめて取り込む、等)。

### 3. 実行

```bash
# GitHub:
gh pr merge <番号> --squash --delete-branch     # 方式に応じて --merge / --rebase
# GitLab:
glab mr merge <番号> --squash --remove-source-branch --yes
# パイプライン完了待ちで自動マージするなら --auto-merge

git switch main && git pull                     # ローカル main を更新
```

### 4. 報告

- 例: 「PR #12 を squash マージしました (ブランチ削除済み・ローカル main 更新済み)」
- マージ失敗 (保護ルール・権限・コンフリクト) は隠さず先頭で、実際の出力とともに
