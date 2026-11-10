# Liquid Glass — Web 実装レシピ

方向性がガラス系のとき、またはユーザーが Liquid Glass / グラスモーフィズムに言及したら Phase 3 の前に必ず読む。
Apple の Liquid Glass (2025) は「半透明レイヤー + 背景ブラー + スペキュラハイライト + 文脈適応」の素材言語。
Web には公式 API が無いので、ここにあるレイヤ構成で再現する。

## 1. 使用規律 — どこに使い、どこに使わないか

ガラスは**コンテンツの上に浮くレイヤーとインタラクティブ要素**にだけ意味がある:

- 使う: 固定ナビ / ヘッダー、フローティングツールバー・ドック、オーバーレイ / シート / ポップオーバー、
  PiP・ミニプレイヤー、カードの hover 浮上状態
- 使わない: 静的な本文カード全面、記事本体、背後に透けて意味のあるコンテンツが無い場所
  (単色の地の上のガラスはただの灰色の箱 — 背後にスクロールするコンテンツ・図版・グラデがあって初めて生きる)
- **ガラスの上にガラスを重ねない**。1 画面のガラス面は 2 枚まで
- ガラス上のテキストは「背後に最悪の背景が流れてきた場合」でもコントラストを保つ — tint の不透明度がその保険。
  半透明はトークン検証の対象外なので、**Phase 4 のスクリーンショット目視で最悪ケースを確認する**
- `prefers-reduced-transparency` / `prefers-contrast` / backdrop-filter 非対応へのフォールバック (§3) は必須

## 2. レイヤ解剖 (6 層)

本物のガラスは「背景を明るく・彩度高く見せ、輪郭に光を集める」。それを CSS で分解すると:

| 層 | 実装 |
| --- | --- |
| ① 材料 (すりガラス) | `backdrop-filter: blur() saturate()` |
| ② 色 (tint) | 半透明の `background` |
| ③ 内側スペキュラ | `box-shadow: inset 0 1px 0` の白ハイライト (上縁に光) |
| ④ エッジ光 | `inset 0 0 0 1px` の半透明白 (全周の稜線) |
| ⑤ ノイズ | `::after` の SVG data URI (opacity 2–3%) |
| ⑥ 浮遊感 | 外向き 2 層シャドウ |

## 3. 基本レシピ `.glass` (完全版・両テーマ)

トークンは :root に置き、テーマブロックで再定義する (color-system.md のデュアルテーマ手順と同じ 3 ブロック構成):

```css
:root {
  --glass-tint: rgb(255 255 255 / 0.55);
  --glass-blur: 20px;               /* モバイルは 12px 目安 (§7) */
  --glass-saturate: 1.5;
  --glass-edge: rgb(255 255 255 / 0.50);   /* 上縁スペキュラ */
  --glass-line: rgb(255 255 255 / 0.12);   /* 全周エッジ光 */
  --glass-shadow: 0 4px 12px rgb(0 0 0 / 0.08), 0 16px 48px rgb(0 0 0 / 0.10);
}
@media (prefers-color-scheme: dark) {
  :root {
    --glass-tint: rgb(22 22 24 / 0.50);
    --glass-saturate: 1.2;                  /* 暗所で彩度を煽らない */
    --glass-edge: rgb(255 255 255 / 0.12);
    --glass-line: rgb(255 255 255 / 0.08);
    --glass-shadow: 0 4px 16px rgb(0 0 0 / 0.40), 0 20px 56px rgb(0 0 0 / 0.45);
  }
}
:root[data-theme="dark"] { /* 同上を再宣言 (手動切替がメディアクエリに勝つ) */ }

.glass {
  position: relative;
  background: var(--glass-tint);
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate));
  backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate));
  border-radius: var(--radius-lg, 16px);
  box-shadow:
    inset 0 1px 0 var(--glass-edge),
    inset 0 0 0 1px var(--glass-line),
    var(--glass-shadow);
}

/* ⑤ ノイズ層 (data URI は rich-techniques.md §4 と同一文字列が正) */
.glass::after {
  content: "";
  position: absolute;
  inset: 0;
  border-radius: inherit;
  pointer-events: none;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='120' height='120'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.8' numOctaves='2'/%3E%3C/filter%3E%3Crect width='120' height='120' filter='url(%23n)' opacity='0.5'/%3E%3C/svg%3E");
  opacity: 0.03;
  mix-blend-mode: overlay;
}
```

ダークでは影がほぼ見えないため、エッジ光 (③④) が支配的になる — 上の変数再定義がその調整。

**フォールバック 3 種 (必須・省略不可)**:

```css
/* backdrop-filter 非対応 → 不透明サーフェスに自動フォールバック */
@supports not ((backdrop-filter: blur(1px)) or (-webkit-backdrop-filter: blur(1px))) {
  .glass { background: var(--color-surface); }
}
/* 透明度を減らす設定のユーザー */
@media (prefers-reduced-transparency: reduce) {
  .glass {
    background: var(--color-surface);
    -webkit-backdrop-filter: none;
    backdrop-filter: none;
  }
}
/* 高コントラスト設定のユーザー */
@media (prefers-contrast: more) {
  .glass { background: var(--color-surface); border: 1px solid var(--color-text-secondary); }
}
```

固定ヘッダーに使う場合は `.glass` + `position: sticky; top: 0;` に、角丸と外影を外す
(`border-radius: 0; --glass-shadow: 0 1px 0 var(--color-border);`) だけでよい。

## 4. インタラクティブ状態

### hover sheen (光沢が 1 回走る)

```css
.glass-sheen { overflow: clip; }
.glass-sheen::before {
  content: "";
  position: absolute;
  inset: 0;
  border-radius: inherit;
  pointer-events: none;
  background: linear-gradient(105deg, transparent 40%, rgb(255 255 255 / 0.18) 50%, transparent 60%)
    no-repeat 120% 0 / 220% 100%;
}
.glass-sheen:hover::before {
  transition: background-position 600ms var(--ease-out);
  background-position: -20% 0;  /* transition を :hover 側にだけ書く = 離脱時は即リセットで逆走しない */
}
```

### press (押し込み + tint 濃度)

```css
.glass-button {
  transition: scale 300ms var(--spring-snappy), background 150ms var(--ease-out);
}
.glass-button:active {
  scale: 0.97;
  --glass-tint: rgb(255 255 255 / 0.68);  /* backdrop-filter は動かさない (§7) — tint 側を動かす */
  transition-duration: 80ms;               /* 押下は即応、復帰はスプリング */
}
```

### focus-visible (ガラス上でも見えるリング)

```css
.glass :is(a, button, [tabindex]):focus-visible {
  outline: 2px solid var(--color-accent-text, var(--color-accent));
  outline-offset: 2px;
}
```

### pill ナビのインジケータ滑走 (SwiftUI の glassEffectID 相当の代替)

Web に GlassEffectContainer は無い。goo フィルタ (feGaussianBlur + feColorMatrix) はテキストが滲み
コストも高いため**不採用**。サイズ・位置のモーフィングは motion.md R7 (View Transitions) / R10 (FLIP) に委譲し、
ナビ内のアクティブ表示は「同一ガラス上のハイライト層の滑走」で作る:

```html
<nav class="glass glass-nav">
  <span class="glass-nav-indicator" aria-hidden="true"></span>
  <button class="is-active" aria-current="page">概要</button><button>仕様</button><button>価格</button>
</nav>
```

```css
.glass-nav { display: flex; gap: 4px; padding: 4px; }
.glass-nav-indicator {
  position: absolute;
  top: 4px;
  left: 0;
  height: calc(100% - 8px);
  width: var(--w, 0);
  translate: var(--x, 0) 0;
  border-radius: 999px;
  background: color-mix(in oklch, var(--color-surface) 70%, transparent);
  box-shadow: inset 0 1px 0 var(--glass-edge), 0 1px 4px rgb(0 0 0 / 0.10);
  transition: translate 400ms var(--spring-snappy), width 400ms var(--spring-snappy);
}
```

```js
const nav = document.querySelector(".glass-nav");
const move = (btn) => {
  nav.style.setProperty("--x", `${btn.offsetLeft}px`);
  nav.style.setProperty("--w", `${btn.offsetWidth}px`);
};
nav.addEventListener("click", (e) => {
  const btn = e.target.closest("button");
  if (!btn) return;
  nav.querySelector(".is-active")?.classList.remove("is-active");
  btn.classList.add("is-active");
  move(btn);
});
move(nav.querySelector(".is-active"));
```

例外の明文化: このインジケータに限り `width` の遷移を許容する (面積が小さく reflow コストが無視できる)。
テキストを含む要素のサイズモーフィングは FLIP / View Transitions を使うこと。

## 5. SVG 屈折版 (上級・Chromium 限定)

feTurbulence を変位マップにして backdrop を歪ませると、すりガラスの「屈折のゆらぎ」が出る。
レンズ状の縁屈折 (エッジで像が曲がる) まで欲しい場合は feImage の放射グラデ変位が必要になるが、
本レシピの範囲外 — ここでは「ゆらぎ」までを扱う。

```html
<svg width="0" height="0" aria-hidden="true"><defs>
  <filter id="glass-refract" x="-10%" y="-10%" width="120%" height="120%">
    <feTurbulence type="fractalNoise" baseFrequency="0.008 0.012" numOctaves="2" seed="7" result="n"/>
    <feDisplacementMap in="SourceGraphic" in2="n" scale="24" xChannelSelector="R" yChannelSelector="G"/>
  </filter>
</defs></svg>
```

```css
.refract-ok .glass--refract {
  -webkit-backdrop-filter: none;
  backdrop-filter: url(#glass-refract) blur(12px) saturate(1.5);
}
```

```js
// 適用ゲートは JS で行う。backdrop-filter: url() の実用対応は 2026 現在 Chromium のみで、
// @supports はパース成功だけで真になり得て信用できない。userAgentData は Chromium 系のみ実装。
// TODO: interop 改善時に @supports ベースへ見直す
if (navigator.userAgentData?.brands?.some((b) => b.brand.includes("Chromium"))) {
  document.documentElement.classList.add("refract-ok");
}
```

非 Chromium は §3 の blur ガラスのまま (自然劣化)。チューニング:

| パラメータ | 意味 | レンジ |
| --- | --- | --- |
| baseFrequency | ゆらぎの粗さ | 0.004 (粗・大きなうねり) 〜 0.02 (細) |
| scale | 歪み量 | 12 (控えめ) 〜 40 (強) |
| numOctaves | ディテール | 2 で十分。3+ は高コスト |

## 6. 色収差 (オプション・要点のみ)

エッジに RGB のズレを出す本物のレンズ表現。構成: §5 のフィルタを複製し、feDisplacementMap を
R / G / B チャネルごとに scale を ±10% ずらして 3 回適用 → `feBlend mode="screen"` で合成。
**コストは極大** — 遊び心が明示要件のときの主役 1 箇所のみ。通常案件では §3–4 で十分。

## 7. パフォーマンス

backdrop-filter は背後の再サンプリングが毎フレーム走る。スクロールコンテンツ直上のガラスが最もコスト高。

- 面積を絞る — 全幅バーは高さを薄く。巨大なガラスパネルを作らない
- blur 半径: デスクトップ 20px / モバイル 12px 目安 (半径がコストに直結)
- ガラスを重ねない (合成レイヤーの爆発)
- `will-change` は付けない — 常時レイヤー化は逆効果
- **backdrop-filter を transition / animation しない** (描画がパカつく)。状態変化は tint (§4 press) で表現する
- 計測: DevTools > Rendering > Frame Rendering Stats を出してスクロール 60fps を確認

## 8. 納品前チェックリスト (ガラスを使ったら)

- [ ] ガラス上テキストが最悪の背景 (明/暗どちらに流れても) で読める — スクショで確認
- [ ] `prefers-reduced-transparency` / `prefers-contrast: more` / 非対応フォールバックの 3 つで成立する
- [ ] ガラスの上にガラスが無い / 1 画面 2 面以内
- [ ] focus-visible リングがガラス上で視認できる
- [ ] スクロールが実機で 60fps (特にモバイル幅)
