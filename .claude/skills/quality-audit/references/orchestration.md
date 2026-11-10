# マルチエージェント編成ガイド

Phase 3 (監査) と Phase 5 (修正) のエージェント編成。実行手段は環境に合わせて選ぶ:

- **Workflow ツールがある**: それを使う (このスキル経由の呼び出しは明示的オプトインに該当)。
  決定論的なループ・fan-out・スキーマ付き出力が得られるため第一選択。
- **ない**: Agent ツールの並列起動 (1メッセージに複数 tool call) で同じ構成を組む。
  各エージェントには JSON で findings を返すよう指示し、自分でパースする。

## 専門エージェント・他スキルの併用

環境に専門のレビュアーエージェント (例: `code-reviewer`, `security-reviewer`,
`python-reviewer`, `typescript-reviewer`, `refactor-cleaner` などの subagent type) が
インストールされている場合は、汎用エージェントに自前プロンプトを渡すより精度が高いことが多いので
積極的に併用する:

- **Layer A/B の補強**: 言語別レビュアー (python-reviewer 等) がいれば、該当言語のチャンクの
  監査エージェントとして使う (Workflow なら `agentType` オプション、Agent ツールなら
  `subagent_type`)。出力形式が findings スキーマと違う場合は受け取った内容を自分でスキーマに正規化する。
- **セキュリティ次元**: `security-reviewer` 系がいれば hygiene/secrets 次元を任せる。
- **Phase 5 の修正後レビュー**: 各カテゴリのコミット前に `code-reviewer` 系エージェント、
  または `/code-review` 相当のスキルが使えるなら diff レビューを挟むとテストでは捕まらない
  リグレッション (ロジック退行・コピペミス) を検出できる。コストとの相談だが、
  risk=medium 以上のカテゴリ (共通化・構造改善) では強く推奨。
- 専門エージェントが**いない場合に探し回らない**こと。このファイルのプロンプトテンプレートで十分機能する。

## チャンク分割の指針

- ディレクトリ・レイヤー単位でまとめる (同じ文脈のファイルを同じエージェントが読むと精度が上がる)。
- 1チャンク = 20〜40 ファイル or 3,000〜5,000 行目安。巨大ファイルは単独チャンク可。
- チャンク ID は人が読める名前にする (`backend-services`, `frontend-hooks` 等)。

## findings スキーマ

全エージェントの出力をこの形に揃える:

```json
{
  "files_reviewed": ["path/a.py", "path/b.py"],
  "findings": [
    {
      "id": "後で採番 (カテゴリ接頭辞 + 連番: DEAD-01, CFG-03 等)",
      "dimension": "conventions|config|dry|structure|dead-code|error-handling|types|deps|hygiene",
      "file": "path/a.py",
      "line": 42,
      "title": "1行要約",
      "description": "何が問題か。根拠 (Grep 結果・参照数等) を含める",
      "proposed_fix": "どう直すか。具体的に (抽出先・設定キー名・削除範囲)",
      "severity": "high|medium|low",
      "risk": "safe|low|medium|high",
      "behavior_change": "none|possible|yes",
      "confidence": "high|medium|low"
    }
  ],
  "utility_signatures": [
    {"file": "path/a.py", "name": "retry_with_backoff", "purpose": "リトライ汎用処理"}
  ],
  "tool_rejections": [
    {"tool": "knip", "item": "unused export foo", "reason": "next.config.js から動的参照されている"}
  ]
}
```

`utility_signatures` は Layer A のみ: チャンク内で見つけた「util 的な処理」(変換・検証・整形・
リトライ・日付処理など、他所にもありそうなもの) の一覧。Layer B の重複検出の入力になる。
`tool_rejections` も Layer A のみ: ツール所見のうち偽陽性と判断して棄却したものと理由。
「ツールが出したのに finding にない」ものが黙殺か棄却かを後から区別できるようにするため。

## エージェントプロンプトのテンプレート

### Layer A — チャンク監査

```
あなたはコード品質監査エージェント。担当チャンクの全ファイルを精読し、findings を JSON で返す。

プロジェクト規約 (要約): <Phase 0 で収集した規約・多数派スタイルをここに貼る>
担当ファイル: <チャンクのファイルリスト>
監査基準: <audit-dimensions.md の該当セクションを貼る (全次元)>
ツール所見 (機械出力・未検証): <Phase 3.0 のツール出力からこのチャンクの該当行を抽出して貼る>

ルール:
- 担当ファイルは全て読むこと。読んだファイルを files_reviewed に必ず列挙する (漏れ検査に使う)
- ツール所見は精読と突合し、正しいものは finding に昇格 (根拠に「ツール名 + 精読で確認」と書く)、
  偽陽性は tool_rejections に棄却理由つきで列挙する。黙って無視しない
- 根拠のない指摘をしない。デッドコード候補は参照を Grep してから報告し、Grep 結果を根拠に書く
- 修正提案は具体的に (「設定化すべき」ではなく「設定キー XXX としてどの設定機構に追加」まで)
- 確信が持てないものは confidence=low で報告してよい (後段で検証する)。黙って捨てない
- 担当外ファイルの修正提案はしない (重複の相手先への言及は OK)
- secrets を発見した場合、値は絶対に転記しない。file:line とキー名、先頭数文字 + マスクのみ
返答は JSON のみ。
```

### Layer B — 横断監査 (次元ごと)

```
あなたは横断監査エージェント。担当次元: <dry / conventions / structure / config の一貫性>
プロジェクト規約 (要約): <同上>
入力: <Layer A の utility_signatures 集約 / 規約ゆらぎの観測メモ>

タスク (dry の例):
- signatures から同目的の処理が複数チャンクにないか調べ、Grep で実装を読み比べて重複を特定する
- 既存の共通 util (utils/, lib/ 等) を把握し、「既にあるのに再実装されている」ものを優先的に探す
監査基準: <audit-dimensions.md の担当次元セクション>
返答は findings JSON のみ。
```

### Layer C — 偽陽性検証 (デッドコード・重複統合・high セキュリティの candidate ごと)

```
あなたは検証エージェント。次の finding が偽陽性でないか反証を試みよ:
<finding JSON>

デッドコードの場合、以下を全て Grep で確認する:
シンボル名の文字列参照 / 動的 import・getattr / ルート・イベント・タスク名としての登録 /
設定ファイル・テンプレート・CI 設定・package.json scripts からの参照 /
フレームワークの規約ベース解決 (ファイル名規約で自動ロードされるものか)。
迷ったら verdict=keep (削除しない方向に倒す)。

severity=high のセキュリティ finding の場合、以下を確認する:
- 到達性: その値は本当に外部入力から届くか (ルーティング・呼び出し元を遡って経路を特定する)
- 既存防御: フレームワーク・ミドルウェアが既に防いでいないか (ORM のパラメータ化、
  自動エスケープ、認証ミドルウェアの適用範囲、バリデーション層)
- 有効性: そのコードパスは本番で有効か (デバッグ専用・テスト専用・フィーチャーフラグで無効化済みでないか)
経路が実在すれば verdict=confirmed + evidence に経路を書く。デッドコードと逆で、
セキュリティは見逃しの方が高くつく — 反証しきれなければ needs-human (severity を維持したまま人間判断へ)。
返答: {"finding_id": "...", "verdict": "confirmed|keep|needs-human", "evidence": "..."}
```

## Workflow スクリプトの骨子

Workflow ツールを使う場合の例。**チャンク定義・プロンプト素材は `args` で渡さず、スクリプト内に
定数として直接埋め込むこと** — `args` は環境によってスクリプトへ届かないことがあり
(`args.chunks.map is undefined` で即落ちする)、埋め込みなら resume 時のキャッシュも安定する。

構成のポイント: チャンク監査 → そのチャンクの findings 検証 は `pipeline()` で流す。
先に終わったチャンクの検証を、遅いチャンクの完了を待たずに始められるため、
wall-clock は「最遅チャンク + その検証」で済む。バリア (`parallel` 待ち合わせ) が正当なのは
Layer B (全チャンクの signatures 集約が入力) だけ。

```js
export const meta = {
  name: 'quality-audit-scan',
  description: 'チャンク監査→即時検証のパイプライン + 横断監査 + カバレッジ集約',
  phases: [{ title: 'Chunk audit' }, { title: 'Verify' }, { title: 'Cross-cutting' }],
}
// データはすべてスクリプト内に定数で埋め込む (ツール所見も chunk.prompt に含めておく)
const CHUNKS = [{ id: 'backend-services', prompt: '...' }, /* ... */]
const CROSS = [{ dimension: 'dry', prompt: '...' }, /* ... */]
const VERIFY_HEADER = '...'
const FINDINGS = { /* findings スキーマ (JSON Schema)。findings[].id は採番不要 (集約時に付与) */ }
const VERDICT = { /* Layer C 返答スキーマ */ }

const needsVerify = f => ['dead-code', 'dry'].includes(f.dimension) ||
  (f.dimension === 'security' && f.severity === 'high')
const verifyAll = (findings, tag) => parallel(findings.map((f, i) => () =>
  agent(VERIFY_HEADER + '\n\n' + JSON.stringify(f),
        { label: `verify:${tag}-${i}`, phase: 'Verify', schema: VERDICT, effort: 'high' })
))

// Layer A → Layer C をチャンクごとに独立に流す (barrier なし)
const chunkResults = (await pipeline(
  CHUNKS,
  c => agent(c.prompt, { label: `audit:${c.id}`, phase: 'Chunk audit', schema: FINDINGS }),
  async (r, c) => {
    if (!r) return null
    const verdicts = (await verifyAll((r.findings || []).filter(needsVerify), c.id)).filter(Boolean)
    return { ...r, verdicts }
  }
)).filter(Boolean)

// barrier が正当: Layer B は全チャンクの signatures 集約が必要
const signatures = chunkResults.flatMap(r => r.utility_signatures || [])
phase('Cross-cutting')
const crossResults = (await parallel(CROSS.map(p => () =>
  agent(p.prompt + '\n\nsignatures:\n' + JSON.stringify(signatures),
        { label: `cross:${p.dimension}`, phase: 'Cross-cutting', schema: FINDINGS })
))).filter(Boolean)
const crossFindings = crossResults.flatMap(r => r.findings || [])
const crossVerdicts = (await verifyAll(crossFindings.filter(needsVerify), 'cross')).filter(Boolean)

return {
  chunk_results: chunkResults,          // findings + verdicts + tool_rejections (チャンク別)
  cross_findings: crossFindings,
  cross_verdicts: crossVerdicts,
  files_reviewed: [...new Set(chunkResults.flatMap(r => r.files_reviewed || []))],
}
```

- 戻り値の `files_reviewed` をマニフェストと突合するのは呼び出し側 (Layer D)。
  未カバーがあれば、そのファイルだけで追加チャンクを作り再実行する
  (`resumeFromRunId` を使えば完了済みチャンクはキャッシュから返り、追加分だけ実行される)。
- 集約時に ID 採番と重複マージ (file + line 近接 + 同一 dimension) を行う。cross と chunk が
  同じ問題を報告していたら1件に統合し、verdict は厳しい方 (keep > needs-human > confirmed) を採る。
- effort: 監査エージェント (Layer A/B) は下げない — コストの大半は読む量であり、
  reasoning を削ると偽陰性が増える。Layer C の検証は反証の徹底が価値なので `effort: 'high'` にする。

## Phase 5 (修正) の並列化

- カテゴリ内でファイル数が多い場合のみ並列化する。**ファイル単位で担当を割り、
  同一ファイルを2エージェントに渡さない** (コンフリクトするため)。
- worktree 分離は不要 (同一ブランチで直列コミットするため)。並列化は「編集作業」だけに留め、
  テストゲートとコミットは必ずオーケストレーター (自分) が直列で行う。
- 修正エージェントには該当 finding と fix-policies.md の該当セクションを渡し、
  「指示された修正以外を行わない」ことを明示する。
