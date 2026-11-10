# Git Workflow

## Commit Message Format

```text
<type>: <description>

<optional body — why, not what>
```

Types: feat, fix, refactor, docs, test, chore, perf, ci

Attribution is disabled globally — no Co-Authored-By / "Generated with" footers.

## Language — git に書く文章は全て日本語

以下は必ず日本語で書く (type — feat, fix… — だけは英語キーワードのまま):

- コミットメッセージの description と本文
- PR / MR のタイトル・本文 (見出しは `## 概要 / ## 変更点 / ## 確認方法`)
- レビューへの返信コメント・Issue コメント

例外 (日本語化の対象外):

- ブランチ名・タグ名 — ASCII (英語ケバブケース) のまま。ツール互換のため日本語にしない
- コードそのもの — 変数名・関数名・クラス名などの識別子は通常どおり英語
  (命名は `code-quality.md` の「既存語彙に合わせる」に従う)。このルールはあくまで
  git に書く「文章」の言語であり、diff の中身には適用しない

## Hosting flows

- GitHub リポジトリ (PR, `gh`) → **github-workflow** スキルを使う
- GitLab リポジトリ (MR, `glab` / push options) → **gitlab-workflow** スキルを使う

Destructive git operations (force-push, history rewrite, deleting branches with
unmerged work) always require explicit user confirmation.
