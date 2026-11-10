# Superpowers 運用ルール

> Source: `claudeknowledge` repo → `rules/superpowers-profile.md`。編集はリポジトリ側で行い、`install.sh` で反映する。

開発プロセス (設計・計画・実装・デバッグ・検証・レビュー) は **Superpowers**
(`superpowers@claude-plugins-official`) に任せる。進め方を決める自作の枠組みは併用しない —
Superpowers のスキルとぶつけないため。このファイルは Superpowers の進め方を変えず、Superpowers に無いものだけを足す。

Superpowers はプラグインなので、マシンごとにインストールされている環境でだけ使える
(claudeknowledge の `install.sh` が入れる)。Web 版のクラウドセッションはプラグインを読み込まないため
`superpowers:*` スキルは無い — その場合もこのファイルと `code-quality.md` は有効。

## 併用するもの (進め方ではなく、個別の基準・作業を担う)

- `code-quality.md` (コードの品質基準)・`git-workflow.md`・`performance.md`
- 独自スキル: `git-*` / `redmine-start` / `quality-audit` / `project-manual` / `user-manual` / `ui-craft`
- Stop hook: `design-first-check.sh` / `root-cause-first-check.sh` (導入されている環境のみ)

## Stop hook と連動する 2 つ (守らないとセッション中 1 回ブロックされる)

1. **不具合修正**: `superpowers:systematic-debugging` で根本原因が確定したら、ソースを編集する**前に**
   返答の中に次の 2 行を書く (`root-cause-first-check.sh` が transcript からこれを確認する)。

   ```text
   ROOT_CAUSE_SITE: src/foo/bar.py:42
   ROOT_CAUSE: <破られた不変条件を 1 文で>
   ```

   場所は「症状が出た箇所」ではなく「不変条件が最初に破られる箇所」。
2. **設計・計画**: 設計書・計画書は Superpowers 既定の `docs/superpowers/specs/` と
   `docs/superpowers/plans/` に置き、保存場所を変えない (`design-first-check.sh` は、ここへの書き込みの後に
   ユーザーの返答があることを承認とみなす)。これらの文書をコミットするかはリポジトリの方針に従う。
   方針が無いチームのリポジトリでは `docs/superpowers/` を `.git/info/exclude` に入れ、コミットしない。

## Superpowers に無い最低限の規律

- **記憶の API は仮説**: 関数・フラグ・パス・設定キーは grep / 型 / 公式ドキュメントで実在確認してから使う。
  確認できないものは「未確認」と書く
- **悪い知らせが先**: 失敗・未検証・ブロッカーは報告の 1 行目に、実際の出力つきで書く。
  検証していないことを検証済みのように書かない
- **テストを弱めて通さない**: テストの削除・skip・特別扱いで green にしない。テストが本当に間違っているなら、
  そう言って独立した変更として直す
- **エラーを握りつぶさない**: 空の catch・`|| true`・失敗を既定値で覆うフォールバックを作らない
- **矛盾は言う**: 証拠がユーザーの認識や自分の過去の発言と食い違ったら、黙ってどちらかで進めない

## 言語

Superpowers のスキル本文は英語だが、ユーザーへの返答・設計書・計画書は日本語で書く
(コミットメッセージ等は `git-workflow.md` に従う)。
