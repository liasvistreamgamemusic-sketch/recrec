# モーション設計 — 質の高いリッチは「振付」で作る

Phase 3 の前に必ず読む。**既定はリッチ — 動きのない UI は未完成として扱う** (ミニマル合意時のみ例外)。

## 1. 原則: 数ではなく振付

リッチなモーションとは動く要素が多いことではない。**1 つの意図を持ったシーケンス (振付) に複数要素が
従うこと**だ。ヒーローで見出し → 本文 → CTA → 図版が 40ms 刻みで立ち上がる 1 回の演出は、
ページ中に散った 20 個のホバーアニメより豪華に見える。

- 判断基準: その動きは「空間の連続性・因果・状態変化」のどれかを説明しているか。していなければ削る
- ページの標準構成: **主役演出 1 (R1 or R3) + セクション出現 (R2) + 全対話要素のマイクロ (R5)**。
  ここまでが下限 (richness floor)、追加演出は上限 2
- 動きの下限 (これが無いと Phase 4 で減点): 全対話要素の hover / press フィードバック、
  ロード演出 1 回、状態変化 (開閉・切替・追加) のトランジション

## 2. トークン

```css
:root {
  --duration-fast: 150ms;   /* マイクロ (hover, press, toggle) */
  --duration-base: 250ms;   /* 標準 (モーダル, パネル, 出現) */
  --duration-slow: 450ms;   /* オーケストレーション (ページロード) */
  --ease-out: cubic-bezier(0.22, 1, 0.36, 1);      /* 入場・出現の既定 */
  --ease-in-out: cubic-bezier(0.65, 0, 0.35, 1);   /* 画面内 A→B の移動 */
  --ease-snappy: cubic-bezier(0.16, 1, 0.3, 1);    /* 小さい UI 部品 */
}
```

### スプリング (実測カーブ・コピペ可)

減衰振動モデルから静定時間いっぱいで 51 点サンプリングした `linear()`。
**カーブと duration はペア (静定時間由来) — 片方だけ変えない**。両方とも `--duration-spring: 500ms` で使う。

```css
:root {
  --duration-spring: 500ms;
  /* フォールバック (linear() 非対応ブラウザ) */
  --spring-gentle: cubic-bezier(0.22, 1, 0.36, 1);
  --spring-snappy: cubic-bezier(0.34, 1.3, 0.5, 1); /* 軽微なオーバーシュート */
}
@supports (animation-timing-function: linear(0, 1)) {
  :root {
    /* gentle: stiffness 170 / damping 24 (ζ=0.92, オーバーシュートなし) — ドロワー・モーダル・パネル */
    --spring-gentle: linear(0, 0.008, 0.03, 0.061, 0.101, 0.146, 0.194, 0.244, 0.295, 0.346, 0.396, 0.445, 0.491, 0.536, 0.578, 0.617, 0.654, 0.688, 0.72, 0.749, 0.776, 0.8, 0.822, 0.843, 0.861, 0.877, 0.892, 0.905, 0.917, 0.928, 0.937, 0.946, 0.953, 0.96, 0.965, 0.97, 0.975, 0.979, 0.982, 0.985, 0.987, 0.99, 0.991, 0.993, 0.994, 0.996, 0.996, 0.997, 0.998, 0.999, 1);
    /* snappy: stiffness 400 / damping 25 (ζ=0.63, ピーク 1.08 = 8% オーバーシュート) — トグル・タブ・小物 UI */
    --spring-snappy: linear(0, 0.019, 0.071, 0.146, 0.235, 0.333, 0.433, 0.532, 0.625, 0.712, 0.789, 0.857, 0.915, 0.963, 1.001, 1.031, 1.052, 1.067, 1.076, 1.08, 1.08, 1.078, 1.072, 1.066, 1.058, 1.05, 1.042, 1.034, 1.027, 1.02, 1.014, 1.009, 1.005, 1.001, 0.999, 0.997, 0.995, 0.994, 0.994, 0.993, 0.994, 0.994, 0.994, 0.995, 0.996, 0.996, 0.997, 0.998, 0.998, 0.999, 1);
  }
}
```

React + Motion (Framer) が使える場合の対応値: gentle `{ stiffness: 170, damping: 24 }` /
snappy `{ stiffness: 400, damping: 25 }`。中断時に慣性が保たれるのは JS 物理のみ。

## 3. 規律 (物理は変えない)

- **動かすのは transform / opacity だけ** (コンポジタ処理)。width / height / top / margin は禁止。
  高さの開閉は `grid-template-rows: 0fr → 1fr`
- 移動距離: マイクロ 4–16px / 出現 20–40px。画面の端から飛んでこさせない
- 退場は入場より速く (入場 300ms なら退場 200ms)。hover の反応は 150ms 以内に見え始める
- **UI 部品への bounce / elastic 禁止** — 例外はスプリングの ≤8% オーバーシュート (--spring-snappy) のみ
- 1s を超えるアニメーションは書かない。`linear` イージングはプログレスバーとスクラブ演出 (R4) のみ
- `animation-timeline` を使うときは easing を linear に (進行がジェスチャに紐づくため)

## 4. 基盤: reduced motion とレベル制御

**全ページ必須** — まずこの安全網を敷いてからレシピを足す:

```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
    scroll-behavior: auto !important;
  }
}
```

より丁寧な実装 (推奨): 移動 (transform) だけ殺して opacity フェードは残す。JS レシピは各自
`matchMedia('(prefers-reduced-motion: reduce)')` で分岐する (R2/R6/R8/R9/R10 に組込み済み)。

**モーションレベルの一括制御**: `<html data-motion="minimal|standard|rich">` を規約にする。
リッチ専用レシピ (R3/R4/R8/R9) は減算式でスコープすると、納品後にワンタッチでレベルを下げられる:

```css
html:not([data-motion="rich"]) .parallax { animation: none; }
```

## 5. レシピ集 R1–R10

各レシピは「完成コード / 使い所 / 使ってはいけない所」の 3 点セット。

### R1. ロードオーケストレーション (主役演出・最も費用対効果が高い)

```css
.reveal {
  opacity: 0;
  transform: translateY(12px);
  animation: reveal var(--duration-slow) var(--ease-out) forwards;
  animation-delay: calc(var(--stagger, 0) * 40ms);
}
@keyframes reveal { to { opacity: 1; transform: none; } }
```

```html
<section class="hero">
  <h1 class="reveal">見出し</h1>
  <p class="reveal" style="--stagger: 1">リード文</p>
  <div class="reveal" style="--stagger: 2"><a class="btn">CTA</a></div>
  <figure class="reveal" style="--stagger: 3">図版</figure>
</section>
```

使い所: ファーストビューの主要素 4–6 個。JS 不要 (SSR / no-JS 安全)。stagger は整数インデックス方式 —
間隔を変えたいときは `* 40ms` の係数 1 箇所だけ。/ 禁止: 全要素への適用、ビューポート外の要素 (R2 を使う)。

### R2. スクロール出現 (IntersectionObserver ユーティリティ)

```js
const io = new IntersectionObserver((entries) => {
  for (const e of entries) {
    if (!e.isIntersecting) continue;
    e.target.classList.add("is-visible");
    io.unobserve(e.target); // 一度きり — スクロールで戻っても再生しない
  }
}, { threshold: 0.15, rootMargin: "0px 0px -10% 0px" });

const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
document.querySelectorAll("[data-reveal], [data-reveal-group]").forEach((el) => {
  if (reduced) { el.classList.add("is-visible"); return; }
  io.observe(el);
});
// グループの子に stagger 連番を自動注入
document.querySelectorAll("[data-reveal-group]").forEach((g) =>
  [...g.children].forEach((child, i) => child.style.setProperty("--stagger", i)));
```

```css
[data-reveal], [data-reveal-group] > * {
  opacity: 0;
  transform: translateY(20px);
  transition: opacity var(--duration-slow) var(--ease-out), transform var(--duration-slow) var(--ease-out);
  transition-delay: calc(var(--stagger, 0) * 40ms);
}
:is([data-reveal], [data-reveal-group]).is-visible,
[data-reveal-group].is-visible > * { opacity: 1; transform: none; }
@media (scripting: none) { /* JS 無効環境は最初から表示 */
  [data-reveal], [data-reveal-group] > * { opacity: 1; transform: none; }
}
```

使い所: セクション単位の出現。`animation-timeline: view()` は**出現トリガには使わず** (IO に一本化)、
スクラブ演出 (R4) 専用 — トリガとスクラブを混ぜると挙動の説明がつかなくなる。

### R3. sticky スクロールシーケンス (主役演出・1 ページ 1 箇所)

```html
<section class="seq">
  <div class="seq-visual" data-step="0">
    <img class="seq-layer" src="a.webp" alt="">
    <img class="seq-layer" src="b.webp" alt="">
    <img class="seq-layer" src="c.webp" alt="">
  </div>
  <div class="seq-steps">
    <div class="step">ステップ 1 の説明</div>
    <div class="step">ステップ 2 の説明</div>
    <div class="step">ステップ 3 の説明</div>
  </div>
</section>
```

```css
.seq { display: grid; grid-template-columns: 1fr 1fr; gap: var(--space-6); }
.seq-visual { position: sticky; top: 20vh; height: 60vh; }
.seq-steps .step { min-height: 90dvh; display: flex; align-items: center; }
.seq-layer {
  position: absolute; inset: 0; opacity: 0; transform: scale(0.98);
  transition: opacity var(--duration-base) var(--ease-out), transform var(--duration-base) var(--ease-out);
}
.seq-visual[data-step="0"] .seq-layer:nth-child(1),
.seq-visual[data-step="1"] .seq-layer:nth-child(2),
.seq-visual[data-step="2"] .seq-layer:nth-child(3) { opacity: 1; transform: none; }
@media (max-width: 767px) { .seq { grid-template-columns: 1fr; } .seq-visual { position: static; height: auto; } }
```

```js
const visual = document.querySelector(".seq-visual");
const steps = [...document.querySelectorAll(".seq-steps .step")];
const seqIo = new IntersectionObserver((es) => {
  for (const e of es) if (e.isIntersecting) visual.dataset.step = steps.indexOf(e.target);
}, { threshold: 0.6 });
steps.forEach((s) => seqIo.observe(s));
```

使い所: プロダクト説明の主役。ステップ 3–5 個。/ 禁止: 1 ページ 2 箇所以上、モバイルでの sticky 維持。

### R4. subtle 視差 (スクラブ演出)

```css
@supports (animation-timeline: view()) {
  .parallax { animation: parallax linear both; animation-timeline: view(); }
  @keyframes parallax {
    from { transform: translateY(5%); }
    to   { transform: translateY(-5%); }
  }
}
```

使い所: 背景図版・装飾レイヤーに ±4–6% だけ。非対応ブラウザは静止 (フォールバック不要の構造)。
禁止: それ以上の振幅 (酔う)、テキストへの適用、**JS の scroll リスナー版視差 (jank の温床 — 書かない)**。

### R5. マイクロインタラクション一式 (全レベル必須)

```css
.btn {
  transition: translate var(--duration-fast) var(--ease-out),
              box-shadow var(--duration-fast) var(--ease-out),
              scale var(--duration-spring) var(--spring-snappy),
              background var(--duration-fast) var(--ease-out);
}
.btn:hover { translate: 0 -1px; box-shadow: var(--shadow-md); } /* 持ち上がる */
.btn:active { scale: 0.98; transition-duration: 80ms; }          /* 押下は即応、復帰はスプリング */
.btn:hover .icon { translate: 3px 0; }
.btn .icon { transition: translate var(--duration-fast) var(--ease-out); }

a { text-underline-offset: 0.2em; text-decoration-thickness: 1px;
    transition: text-decoration-thickness var(--duration-fast), text-underline-offset var(--duration-fast); }
a:hover { text-decoration-thickness: 2px; text-underline-offset: 0.3em; }

:is(input, textarea, select) {
  border: 1px solid var(--color-text-secondary);
  transition: border-color var(--duration-fast), box-shadow var(--duration-fast);
}
:is(input, textarea, select):focus-visible {
  outline: none;
  border-color: var(--color-accent-text, var(--color-accent));
  box-shadow: 0 0 0 3px color-mix(in oklch, var(--color-accent) 25%, transparent);
}
```

「持ち上がって押し込める」物理的一貫性が目的。影のランクは rich-techniques.md §2 のトークンを使う。
disabled は動かさない。ホバーでの画像 scale 拡大は惰性でやらない (anti-ai-checklist)。

### R6. カウントアップ数値

```js
const fmt = new Intl.NumberFormat("ja-JP");
const reducedCount = matchMedia("(prefers-reduced-motion: reduce)").matches;
const countIo = new IntersectionObserver((es) => {
  for (const e of es) {
    if (!e.isIntersecting) continue;
    countIo.unobserve(e.target);
    const to = parseFloat(e.target.dataset.countTo);
    const dur = Number(e.target.dataset.countDuration ?? 600);
    const t0 = performance.now();
    const tick = (t) => {
      const p = Math.min((t - t0) / dur, 1);
      e.target.textContent = fmt.format(Math.round(to * (1 - (1 - p) ** 3))); // ease-out cubic
      if (p < 1) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }
}, { threshold: 0.6 });
document.querySelectorAll("[data-count-to]").forEach((el) => {
  if (reducedCount) return;                       // reduced: マークアップの最終値をそのまま表示
  el.textContent = fmt.format(0);
  countIo.observe(el);
});
```

HTML は `<span data-count-to="12480">12,480</span>` — **マークアップに最終値を書く** (no-JS / reduced で
そのまま見える)。CSS に `font-variant-numeric: tabular-nums` 必須 (桁揺れ防止)。主役数字 1 つだけに使う。

### R7. View Transitions (状態・ページ遷移の連続性)

```js
// 同一ドキュメント: 状態変化を包むだけ。非対応・reduced-motion は即時実行に自然劣化
function withTransition(update) {
  if (!document.startViewTransition || matchMedia("(prefers-reduced-motion: reduce)").matches) {
    update();
    return;
  }
  document.startViewTransition(update);
}
// 例: withTransition(() => { document.documentElement.dataset.theme = "dark"; });
// 例: withTransition(() => { list.prepend(newItem); });
```

```css
::view-transition-old(root), ::view-transition-new(root) {
  animation-duration: var(--duration-base);
  animation-timing-function: var(--ease-out);
}
/* 要素の連続性 (サムネ → 詳細): 遷移の両側で同じ名前を振る */
.thumb--active { view-transition-name: hero-image; }
```

MPA なら CSS 2 行で画面間遷移が付く: `@view-transition { navigation: auto; }` + 両ページの対応要素に
同名の `view-transition-name`。使い所: テーマ切替・リスト並び替え・詳細展開。短く・ease-out・上書きは root のみ。

### R8. テキスト分割 reveal (見出し 1 箇所限定)

```js
// 語/行単位のみ (文字単位のバラバラ・回転入場は禁止 — 2015 年の LP になる)
function splitReveal(el) {
  if (!Intl.Segmenter) return; // 未対応環境は分割せずフェードに劣化
  const text = el.textContent;
  const words = [...new Intl.Segmenter("ja", { granularity: "word" }).segment(text)];
  el.setAttribute("aria-label", text); // 支援技術には元テキストを渡す
  el.textContent = "";
  const wrap = Object.assign(document.createElement("span"), { ariaHidden: "true" });
  words.forEach((w, i) => {
    if (!w.segment.trim()) { wrap.append(w.segment); return; }
    const mask = document.createElement("span");
    mask.className = "split-mask";
    const word = document.createElement("span");
    word.className = "split-word";
    word.style.setProperty("--stagger", i);
    word.textContent = w.segment;
    mask.append(word);
    wrap.append(mask);
  });
  el.append(wrap);
}
```

```css
.split-mask { display: inline-block; overflow: clip; padding-block: 0.1em; margin-block: -0.1em; }
.split-word {
  display: inline-block;
  translate: 0 110%;
  animation: split-in 600ms var(--ease-out) forwards;
  animation-delay: calc(var(--stagger) * 40ms);
}
@keyframes split-in { to { translate: 0 0; } }
@media (prefers-reduced-motion: reduce) { .split-word { translate: 0 0; animation: none; } }
```

### R9. 磁気ボタン (遊び心が明示要件のときだけ・1 コンポーネント)

```js
if (matchMedia("(hover: hover) and (pointer: fine)").matches &&
    !matchMedia("(prefers-reduced-motion: reduce)").matches) {
  document.querySelectorAll(".magnetic").forEach((el) => {
    el.addEventListener("pointermove", (e) => {
      const r = el.getBoundingClientRect();
      const x = Math.max(-6, Math.min(6, (e.clientX - r.x - r.width / 2) * 0.2));
      const y = Math.max(-6, Math.min(6, (e.clientY - r.y - r.height / 2) * 0.2));
      el.style.translate = `${x}px ${y}px`;
    });
    el.addEventListener("pointerleave", () => { el.style.translate = "0 0"; });
  });
}
```

```css
.magnetic { transition: translate var(--duration-spring) var(--spring-gentle); }
.magnetic:hover { transition: translate 80ms linear; } /* 追従中は即応、離れたらスプリングで帰還 */
```

### R10. FLIP (レイアウト変化を transform で滑らかに)

```js
// 並び替え・タブ切替・グリッド変更。View Transitions (R7) が使える場面ではそちらを優先
function flip(elements, mutate) {
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) { mutate(); return; }
  const first = elements.map((el) => el.getBoundingClientRect());
  mutate(); // DOM を実際に変更する
  elements.forEach((el, i) => {
    const last = el.getBoundingClientRect();
    const dx = first[i].x - last.x;
    const dy = first[i].y - last.y;
    if (!dx && !dy) return;
    el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }],
      { duration: 350, easing: "cubic-bezier(0.22, 1, 0.36, 1)" });
  });
}
```

## 6. レベル別マッピング

| レベル | 使うレシピ |
| --- | --- |
| ミニマル (合意時のみ) | R5 のうち色/影のフェードのみ |
| スタンダード | R1, R2, R5, R6, R7 |
| **リッチ (既定)** | R1, R2, R5, R6, R7 + **R3 or R8 (主役演出としてどちらか 1 つ)** + R4 + R10 |
| + 遊び心が明示要件 | + R9 (1 箇所) |

どのレベルでも主役演出は 1 ページ 1 つ。超えて足したくなったら、それは設計ではなく不安 — 削る。
