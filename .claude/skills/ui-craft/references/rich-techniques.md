# リッチ表現レシピ — 素材と設えで「シンプルなのに豪華」

**リッチが既定。ミニマル合意時以外は必ず読む** (モーションは motion.md、ガラスは liquid-glass.md が正)。
前提: リッチさは装飾の物量ではなく、**素材感・タイポの迫力・深度の言語・1 箇所の主役演出**で出す。
どのレシピも anti-ai-checklist.md の両方向 (過剰/不足) の制約内で使う。

## 1. 大型タイポグラフィ (Apple 流・コスト最小)

ファーストビューは「巨大な見出し + 短い 1 文 + CTA 1 つ」まで削る。サイズ差が豪華さを作る:

| 役割 | 値 | 実寸 |
| --- | --- | --- |
| display (ヒーロー) | `clamp(2.75rem, 1.2rem + 6.5vw, 6rem)` | 44 → 96px |
| h1 | `clamp(2rem, 1.1rem + 3.8vw, 3.5rem)` | 32 → 56px |
| h2 | `clamp(1.5rem, 1.2rem + 1.6vw, 2.25rem)` | 24 → 36px |
| lead (リード文) | `clamp(1.125rem, 1rem + 0.5vw, 1.3125rem)` | 18 → 21px |

- display は `letter-spacing: -0.02em〜-0.04em` / `line-height: 1.0–1.1` / `text-wrap: balance`
- 見出しの一部だけ `--color-accent-text` (無ければ accent) で着色 — グラデーションテキストにしない
- Apple の Large Title 縮小 (スクロールで見出しが縮んでヘッダーに納まる) — 主役演出 1 箇所として:

```css
@supports (animation-timeline: scroll()) {
  .hero-title {
    animation: shrink-title linear both;
    animation-timeline: scroll();
    animation-range: 0 240px;      /* 最初の 240px のスクロールで縮み切る */
    transform-origin: left top;
  }
  @keyframes shrink-title { to { transform: scale(0.55); } }
}
```

非対応ブラウザは縮まないだけ (自然劣化)。

## 2. 多層シャドウシステム (深度の言語)

影は ambient (近く柔らかい) + key (遠く広い) の 2 層合成をトークンにする:

```css
:root {
  --shadow-sm: 0 1px 2px rgb(0 0 0 / 0.05), 0 2px 8px rgb(0 0 0 / 0.04);
  --shadow-md: 0 2px 4px rgb(0 0 0 / 0.05), 0 8px 24px rgb(0 0 0 / 0.07);
  --shadow-lg: 0 4px 8px rgb(0 0 0 / 0.06), 0 24px 64px rgb(0 0 0 / 0.10);
  /* 色付き影: アクセント面の要素だけ。黒でなく低彩度のアクセント hue (H はアクセントに合わせる) */
  --shadow-accent: 0 8px 24px oklch(0.25 0.05 32 / 0.25);
}
@media (prefers-color-scheme: dark) {
  :root {
    --shadow-sm: 0 1px 2px rgb(0 0 0 / 0.40);
    --shadow-md: 0 4px 16px rgb(0 0 0 / 0.50);
    --shadow-lg: 0 12px 40px rgb(0 0 0 / 0.60);
  }
}
```

- hover の「浮上」は `--shadow-md → --shadow-lg` のトークン差し替え (motion.md R5 と接続)
- **ダークでは影がほぼ見えない** — 深度は影でなく surface の明度差 (bg +0.03–0.05 L) と border で作る。
  ダーク側の影は「接地感」の補助に留める
- 同一カードにヘアラインボーダー + 大きい拡散影の両掛けは禁止 (anti-ai-checklist)。カードは
  「border のみ (フラット寄り)」か「影のみ (浮遊寄り)」のどちらかに寄せ、ページ内で言語を統一する

## 3. 角丸言語

| 値 | 用途 |
| --- | --- |
| 8px | chip / badge / 小さなタグ |
| 10–12px | button / input |
| 16px | card / パネル |
| 20px | modal / sheet / ポップオーバー |
| 28px | hero 図版・最大面 |

- **同心円規則: 内側の radius = 外側の radius − padding**。ネストでこれがズレると一気に素人感が出る
  (例: 外 16px・padding 8px なら内側は 8px)
- 小さい要素への 24px+ の過剰角丸は禁止 (anti-ai-checklist)
- Apple 的スクワークルにしたい場合 (実験的機能・任意): `@supports (corner-shape: squircle) { .card { corner-shape: squircle; } }`

## 4. ノイズ・テクスチャ・自作図版 (フラット単色からの脱却)

背景が完全な単色 1 枚は richness floor 減点。最低 1 つの素材感を敷く:

```css
/* (a) 粒子ノイズ — data URI はこの文字列が正 (liquid-glass.md §3 と同一) */
.grain { position: relative; }
.grain::after {
  content: "";
  position: absolute;
  inset: 0;
  pointer-events: none;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='120' height='120'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.8' numOctaves='2'/%3E%3C/filter%3E%3Crect width='120' height='120' filter='url(%23n)' opacity='0.5'/%3E%3C/svg%3E");
  opacity: 0.03;
  mix-blend-mode: overlay;
}

/* (b) 細罫パターン (エディトリアルな設え) */
.ruled { background: repeating-linear-gradient(90deg, var(--color-border) 0 1px, transparent 1px 8px); }

/* (c) ドットグリッド (ダッシュボードの地) */
.dotted {
  background-image: radial-gradient(var(--color-border) 1px, transparent 1px);
  background-size: 24px 24px;
}
```

「気配」の背景グラデ (ΔL 0.02–0.04) は color-system.md §7。

**図版の自作**: 画像素材が無いとき、灰色ボックスではなく CSS で作る。アクセントの analogous (H ±20°)
に制限した radial-gradient の重ねが最も安全 (紫系へ寄せない — color-system.md §7):

```css
.figure-abstract {
  background:
    radial-gradient(60% 80% at 20% 10%, oklch(0.85 0.06 45 / 0.55), transparent 60%),
    radial-gradient(50% 70% at 85% 30%, oklch(0.80 0.08 25 / 0.45), transparent 60%),
    radial-gradient(70% 60% at 50% 100%, oklch(0.90 0.04 60 / 0.60), transparent 70%),
    var(--color-surface);
  border-radius: 28px;
}
```

## 5. bento グリッド (均一カードグリッドの代替)

均一 3 枚グリッドの回避は**セルサイズの差**で達成する。gap は全体で統一し、密度差はセル内 padding で作る:

```css
.bento {
  display: grid;
  grid-template-columns: repeat(6, 1fr);
  gap: var(--space-2);
}
.bento > * {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 16px;
  padding: var(--space-3);
}
.bento > .lead {           /* 主役セル: 大きく・余白も厚く */
  grid-column: span 4;
  grid-row: span 2;
  padding: var(--space-6);
}
.bento > .sub { grid-column: span 2; }
@media (max-width: 640px) {
  .bento { grid-template-columns: 1fr; }
  .bento > .lead { grid-column: auto; grid-row: auto; }
}
```

主役セルには display 級の数字か図版を 1 つ。全セル同格になったらそれはただのカードグリッド — 主従を作り直す。

## 6. 罫線エディトリアル (カード無しの高級感)

水平罫 1px だけでセクションや行を区切ると、カード無しで高級感が出る。行 hover は背景色でなく
**内側要素のインデント + 矢印の出現**で応答する:

```css
.rows > a {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-block: var(--space-3);
  border-top: 1px solid var(--color-border);
}
.rows > a:last-child { border-bottom: 1px solid var(--color-border); }
.rows .row-body { transition: translate var(--duration-fast) var(--ease-out); }
.rows > a:hover .row-body { translate: 12px 0; }
.rows .row-arrow {
  opacity: 0;
  translate: -8px 0;
  transition: opacity var(--duration-fast) var(--ease-out), translate var(--duration-fast) var(--ease-out);
}
.rows > a:hover .row-arrow { opacity: 1; translate: 0 0; }
```

## 7. 数字・データの見せ方

- 統計は「大きい数字 ×3 のバナー」ではなく、**1 つの主役数字 + 補助文** (anti-ai-checklist)
- 表・カウンタ・時刻には `font-variant-numeric: tabular-nums` 必須 (桁揺れ防止)
- 単位は数字の 0.4–0.5em で小さく添える。カウントアップは motion.md R6 (主役数字 1 つだけ)

```css
.stat {
  font-variant-numeric: tabular-nums;
  font-size: clamp(3rem, 2rem + 4vw, 5rem);
  font-weight: 700;
  letter-spacing: -0.02em;
}
.stat small { font-size: 0.45em; font-weight: 500; color: var(--color-text-secondary); }
```

- チャートは色だけに意味を持たせない (形状・ラベル併用 — color-system.md §5/§6)

## 8. ダークフルブリードセクション (全面ダークにしない代替)

ライト基調の中に**ダークのフルブリードを 1 つ**挟むと緩急でリッチに見える。実装はトークン再定義のみ —
ダークトークンブロックのセレクタを `[data-theme="dark"]` にしておけば (color-system.md §2)、
セクションに属性を付けるだけで反転する:

```html
<section class="fullbleed" data-theme="dark">…</section>
```

```css
.fullbleed {
  margin-inline: calc(50% - 50vw);              /* コンテナを突き破る */
  padding-inline: max(var(--space-4), calc(50vw - 50%));  /* 中身は元のカラムに戻す */
  background: var(--color-bg);
  color: var(--color-text);
}
```

ハードコードで色を差し替えない。中では影 → border/明度差の切替に注意 (§2)。1 ページ 1 箇所。

## 9. ガラス・ブラー

references/liquid-glass.md へ (基本形・インタラクティブ状態・屈折・フォールバックすべてそちらが正)。

## 配合表 (リッチ既定時の標準構成)

| 区分 | 内容 |
| --- | --- |
| **必須 (下限 = richness floor)** | 主役演出 1 (motion.md R1 / R3 / R8 のどれか) + セクション出現 (R2) + 全対話要素のマイクロ (R5) + 素材感 1 種以上 (§4 / 気配グラデ / ガラス) + 深度の言語 (§2 影 or border 明度差) |
| **上限** | 追加演出は 2 まで (視差 R4・カウントアップ R6・View Transitions R7 など)。超えて足したくなったら、それは設計ではなく不安 — 削る |

下限を割ると「無難で平凡」(richness floor 減点)、上限を超えると「全部が動く」(アンチ AI 減点)。この帯の中で振り付ける。
