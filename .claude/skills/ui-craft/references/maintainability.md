# 修正しやすい UI の作り方 — トークン駆動アーキテクチャ

ui-craft の納品物は「ユーザーが自分で (または軽い指示で) 安全に微調整できる」ことが要件。
そのための構造規則。

**適用範囲**: このファイルのルールは「納品済み UI への微調整リクエスト」に適用する。
「デザイン改善して」「リッチに」「見やすく」のような**品質向上依頼は改善モード (redesign-mode.md) の管轄** —
そこでは「トークンのみ」制約を適用してはならない (visible-delta contract が優先)。

## 原則: 変更の 9 割はトークン 1 行で済むようにする

納品後に来る修正リクエストの大半は決まっている:

| よくあるリクエスト | 触る場所 (これ以外を触らせない) |
| --- | --- |
| 「色をもう少し○○に」 | `--color-accent` など color トークン |
| 「もっと詰めて / 余白ほしい」 | `--space-*` (セクション余白は `--space-8`〜`12`) |
| 「角丸をもっと / やめて」 | `--radius-*` |
| 「アニメーション速く / 控えめに」 | `--duration-*` / モーションレベルのクラス |
| 「フォント変えたい」 | `--font-display` / `--font-body` |
| 「ダークモードにしたい」 | 初回納品時から定義済み (color-system.md §2)。調整はダークブロックのトークンのみ |
| 「もっとリッチに / デザイン改善して」 | トークンでは対応しない — **改善モードに再入** (redesign-mode.md) |

逆に言うと、**個別のコンポーネント CSS に生の hex・px 余白・ms を書いた時点でこの表は崩れる**。
実装中に `#`, `px` の余白, `ms` を直書きしそうになったら、それはトークンにすべき値。

## ファイル構成

プレーン CSS / CSS Modules の場合:

```
styles/
  tokens.css        ← 全トークン定義。ユーザーが触るのは原則ここだけ
  base.css          ← reset + タイポグラフィ基礎 (トークン参照のみ)
  components/*.css  ← コンポーネント別 (トークン参照のみ)
```

Tailwind の場合: `@theme` (v4) / `tailwind.config` の `theme.extend` (v3) にトークンを集約し、
任意値 (`mt-[13px]`, `text-[#8b5cf6]`) を使わない。任意値はトークン化の失敗シグナル。

React 系はコンポーネントを「見た目 (トークン参照)」と「データ・ロジック (props)」に分け、
文言・数値・画像 URL はすべて props またはデータ定数ファイル (`content.ts` 等) に出す —
「文言直して」がマークアップを触らず済む。

## ダークモード / テーマ切り替えを見越したトークン設計

色トークンは**役割名**で命名する (`--color-accent`, `--color-surface`)。
`--blue-500` のような値名にすると、テーマ切り替えで名前と実体が乖離する。

```css
:root { --color-bg: #fdfcfb; --color-text: #1a1516; /* ... プリセットの Light 列 */ }

@media (prefers-color-scheme: dark) {
  :root { --color-bg: #161414; --color-text: #f4f1f0; /* ... プリセットの Dark 列 */ }
}
[data-theme="dark"] { /* 手動切替・セクション局所反転用の再宣言 (color-system.md §2) */ }
```

テーマ数の既定: **ブランドアンカー採用時はアンカーのネイティブモードのみ** (両テーマは合意時のみ導出)。
アンカー無し (プリセット採用) 時はライト・ダーク両テーマを初回から定義する (プリセットが両方の検証済み値を持つ)。
色トークンを変えたら `node scripts/validate-palette.mjs <tokens>` で必ず再検証する。

## モーションレベルの一括制御

Phase 1 で合意した「モーション量」を後から変えられるように、動きは data 属性 1 つで畳めるようにする:

```css
html[data-motion="minimal"] .reveal { transition: opacity var(--duration-base) ease; transform: none; }
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after { animation-duration: 0.01ms !important; transition-duration: 0.01ms !important; }
}
```

## 微調整ガイド (報告テンプレ)

納品報告には、上の「よくあるリクエスト」表を**そのプロジェクトの実ファイルパス・実トークン名**で
具体化して載せる。汎用表のコピペではなく、「`src/styles/tokens.css:12` の `--color-accent: #0d6b5c`」
のように 1 行変更で試せる粒度で書く。
末尾に必ず 1 行添える: 「色を変えたら `node ~/.claude/skills/ui-craft/scripts/validate-palette.mjs <tokens>`
でコントラストを再検証してください」。

## やってはいけない

- コンポーネント間で微妙に違う値の複製 (`gap: 14px` と `gap: 16px` が意図なく混在) —
  同じ意図の値は同じトークン
- `!important` での上書き合戦 — 詳細度は低くフラットに保つ (BEM か Tailwind か、どちらかに寄せる)
- インラインスタイルへの生値埋め込み (動的値が必要なら CSS variable を inline で渡す:
  `style={{ '--delay': i * 80 + 'ms' }}`)
- 1 ファイル 800 行超の CSS — セクション/コンポーネント単位で割る
