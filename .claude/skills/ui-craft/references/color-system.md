# 配色システム — 人の目に合わせて色を組む

Phase 2 (トークン定義) の前に必ず読む。このファイルの結論は 2 つ:
**色は手で選ばずシステムで導く**こと、そして**目視ではなく `scripts/validate-palette.mjs` で機械検証する**こと。
プリセット (§8) は全値検証済み — そのまま `:root` に転記すれば「人の目に見やすい」が最初から成立する。

## 1. OKLCH — L / C / H の 3 つのつまみ

`oklch(L C H)`: **L = 知覚明度** (0–1。コントラストをほぼ支配)、**C = 彩度** (0–0.37 程度。上限は hue 依存)、
**H = 色相角** (0–360)。OKLCH の価値は「L が同じ 2 色は同じ明るさに見える」の 1 点。
HSL は同じ L でも yellow と blue の明るさが全く違うため、HSL でランプや対を組むと必ず破綻する。

- 濃淡ランプは **L を等間隔に刻む** (例: 0.98 → 0.15 を 10–12 段)。C はミッドトーンで最大、両端で絞る。hue は固定
- 高 C は sRGB の外にクリップされることがある — DevTools のガモット警告を確認する
- 互換性のため、トークンの実値は hex で書いてよい (プリセットは hex で提供)。設計と検証を OKLCH で行う

## 2. セマンティックトークンと L レシピ

トークンは役割名で 9 つ (surface-2 と accent-text は任意):

```text
--color-bg / --color-surface / --color-surface-2 / --color-text / --color-text-secondary
--color-border / --color-accent / --color-on-accent / --color-accent-text
```

役割別の L 目標値 (プリセット群からの帰納値。自作パレットはこの帯に入れる):

| トークン | Light L | Dark L | C (ニュートラル) |
| --- | --- | --- | --- |
| bg | 0.965–0.99 | 0.18–0.22 (**純黒禁止**) | 0.002–0.016 |
| surface | bg +0.01–0.03 (白側) | bg +0.03–0.05 | 同上 |
| border (装飾罫) | 0.90–0.93 | 0.28–0.33 | 同上 |
| text-secondary | 0.47–0.51 | 0.74–0.78 | ≤ 0.03 |
| text | 0.20–0.29 | 0.93–0.96 (**純白禁止**) | ≤ 0.03 |

規則:

- **ニュートラルの hue バイアス**: 全ニュートラルにアクセント同系の hue を C 0.002–0.03 で乗せる。
  C=0 の純グレーは「無考慮」に見える。例外: surface だけは純白 `#ffffff` 可 (色付き bg との対比で紙の白さを出す)
- **hover 色の導出**: ライトは L −0.05、ダークは L +0.04。どちらもコントラストが上がる方向なので再検証不要
- **入力欄など「境界線だけが手がかり」の部品**は `--color-border` でなく `--color-text-secondary` を使う
  (4.5:1 保証済みなので UI 部品の 3:1 を自動で満たす)。`--color-border` は装飾罫専用
- ダークはライトの機械反転ではなく再選定: C を 10–20% 下げ (ネオン化防止)、影ではなく明度差と border で深度を作る

デュアルテーマは 3 ブロック構成。コンポーネントは常にトークン経由 — メディアクエリ内で直接スタイルしない:

```css
:root { /* ライトのトークン一式 */ }
@media (prefers-color-scheme: dark) {
  :root { /* ダークのトークンのみ再定義 */ }
}
/* 手動切替 + セクション局所反転の両対応:
   html に付ければ全体、セクションに付ければその範囲だけ反転 (rich-techniques.md §8) */
[data-theme="dark"] { /* ダーク再宣言 */ }
:root[data-theme="light"] { /* ライト再宣言 (OS ダーク設定に勝つ) */ }
```

**初回納品からライト・ダーク両方を定義する** (プリセットが両方の値を持つので追加コストはほぼゼロ)。

## 3. アクセント色の選び方

- **彩度フロア: C ≥ 0.10** (validator が FAIL にする)。下回るとグレーに混ざり、アクセントの仕事 (視線誘導) を失う
- 役割別 L バンド:

| 役割 | L 目安 | 根拠 |
| --- | --- | --- |
| 塗り + 白文字 (light) | 0.42–0.58 | on-accent 白の 4.5:1 (朱系は L 0.575 が上限実測) |
| 塗り + 黒系文字 | 0.72–0.88 | 濃色ラベルの 4.5:1 (琥珀・ライム系はこちら) |
| リンク文字 (light bg 上) | ≤ 0.55 | bg との 4.5:1 |
| ダークモードの accent | 0.65–0.78、C はライト比 −10〜20% | 暗地 4.5:1 + ネオン化防止 |

- **明るいアクセント (琥珀・ライム等) は「塗り専用」**: bg との比が 3:1 に届かないため、リンク・フォーカスリング・
  アイコンには濃色の `--color-accent-text` を別途導出して使う (プリセット P4/P5 が実例)。
  validator は accent-text があるとき accent/bg < 3:1 を WARN に緩和し、accent-text/bg ≥ 4.5:1 を FAIL ゲートにする
- アクセントが地と喧嘩したら**色を替えず** analogous (H ±30°) へ寄せるか C を落とす
- dataviz スキルの L バンド (light 0.43–0.77 / dark 0.48–0.67) は**チャート系列色**用 — UI の塗りには上表を使う

## 4. コントラスト基準 — WCAG をフロアに、APCA を品質目標に

| 対象 | WCAG (機械ゲート・非交渉) | APCA Lc (品質目標) |
| --- | --- | --- |
| 本文 (〜18px) | 4.5:1 | ≥ 75 (理想 90) |
| 大文字 (24px+ / 18.5px bold)・補助テキスト | 3:1 (補助が本文サイズなら 4.5:1) | ≥ 60 |
| 見出し 36px+・UI 部品・フォーカスリング・アイコン | 3:1 | ≥ 45 |
| placeholder / disabled | — | ≥ 30 |

- APCA は極性を区別する — **ダークモードの明るい細字は同じ比率でも滲む**。Lc が目標を割るなら weight か size を 1 段上げる
- ライト/ダークは**それぞれ独立に**検証する。検証は目視でなく機械で:

```bash
node scripts/validate-palette.mjs styles/tokens.css   # FAIL が 1 つでもあれば exit 1
```

## 5. 意味色 (status) — アクセントと分離する

good / warning / critical はアクセントの hue と無関係に固定する (全値検証済み):

| status | マーク (light) | テキスト (light・対白) | テキスト (dark・対 #161414) |
| --- | --- | --- | --- |
| good | #0ca30c (3.35:1) | #0a7d0a (5.32:1) | #4ec14e (7.92:1) |
| warning | #fab219 (淡色バッジ形式のみ) | #8a5a00 (5.93:1) | #fab219 (10.0:1) |
| serious | #ec835a (同上) | #b0512b (5.18:1) | #ec835a (6.96:1) |
| critical | #d03b3b (4.8:1) | #b52d2d (6.22:1) | #e05252 (4.8:1) |

- **色単独に意味を持たせない — icon + label 必須** (色覚多様性と国際化の両方の理由)
- warning / serious の生マークは対白 3:1 未満 → 単独ドット禁止。「淡色 tint 背景 + 濃色テキスト」のバッジ形式で使う
- **hue 衝突規則**: アクセント H が status H ±25° に入るプリセットでは、その status に色面を使わず
  アイコン+ラベル形式に固定する (P4 琥珀 → warning、P5 夜光 → good が該当。§8 の注記参照)

## 6. 色覚多様性 (CVD)

UI は「色 + 形状/ラベル」の二重符号で原則クリアする (§5 の icon+label 必須がそれ)。
赤/緑の対比だけで状態を分けない。隣接色ペア自体が意味を運ぶチャートは dataviz スキルの基準
(Machado-2009 シミュレーション下 ΔE ≥ 12) と検証済みパレットに委譲する。

## 7. グラデーションの正しい作り方

1. 基本は**同一 hue の L ランプ** (ΔL 0.06–0.15、C ほぼ一定)。これだけで「AI グラデ」から離陸する
2. hue をまたぐなら **±30° (analogous) まで**。紫→青はアンチ AI チェックリスト該当で禁止
3. 補間は `in oklch` 指定 — sRGB 補間の灰色デッドゾーンを回避 (非対応ブラウザでは指定が無視され通常補間になるだけ)
4. 背景の「気配」グラデは ΔL 0.02–0.04 に留める (見えるか見えないかの質感が目的)

```css
/* 背景の気配 (陶土の例) */
background: linear-gradient(in oklch, oklch(0.975 0.008 55), oklch(0.955 0.012 55));
/* アクセントボタンの光沢 (同一 hue ΔL 0.08) */
background: linear-gradient(in oklch, oklch(0.60 0.19 32), oklch(0.52 0.21 32));
```

## 8. プリセットパレット 6 種 (検証済み実値)

全値は validate-palette.mjs で全ペア PASS 済み (WCAG フロア + APCA 目標 + 彩度バンド)。
使い方: 選ばれたプリセットの Light / Dark 列を Phase 2 の `:root` / dark ブロックにそのまま転記する。
hue だけ変えたい場合は OKLCH の H のみ回して L/C を保ち、必ず validator を通し直す。
系統は意図的に分散してある (AI 的中央値である青・紫を外してある — 青が必要なら §9)。

### P1 墨と朱 (Mono Edge) — 高コントラスト黒 + 鮮烈な朱。エディトリアル / ポートフォリオ / テック

| トークン | Light | Dark |
| --- | --- | --- |
| bg / surface | #fdfcfb / #ffffff | #161414 / #1e1c1d |
| text / text-secondary | #1a1516 / #5f5a58 | #f4f1f0 / #b8b2b0 |
| border | #e8e6e4 | #363234 |
| accent / on-accent | #d92d0c / #ffffff | #f4795c / #2b1209 |

accent は対 bg 4.7:1 — リンク兼用可。critical (#d03b3b) と近縁のため、destructive 操作は赤面でなく形+ラベルで区別する。

### P2 陶土 (Warm Earth) — 温かいアーストーン。クラフト / 食 / ウェルネス

| トークン | Light | Dark |
| --- | --- | --- |
| bg / surface | #f7f3ec / #fffdf8 | #1d1814 / #282219 |
| text / text-secondary | #2b241d / #6e6259 | #f3efe9 / #b9b3aa |
| border | #e6ddcf | #38312a |
| accent / on-accent | #a4552e / #ffffff | #e39668 / #33150c |

### P3 深緑 (Deep Petrol) — 深いエメラルドペトロール。SaaS / 金融 / サステナビリティ

| トークン | Light | Dark |
| --- | --- | --- |
| bg / surface | #f4f7f6 / #ffffff | #0f1a17 / #17241f |
| text / text-secondary | #152420 / #4f625d | #eef4f1 / #a7b8b2 |
| border | #dbe4e1 | #29352f |
| accent / on-accent | #00805f / #ffffff | #3ec2a8 / #072019 |

good (#0ca30c) と hue 近縁 → good は必ずチェックアイコン併用 (§5 hue 衝突規則)。

### P4 琥珀 (Cream & Amber) — クリーム地 + 暖色アンバー。メディア / ホスピタリティ

| トークン | Light | Dark |
| --- | --- | --- |
| bg / surface | #fbf5ea / #fffcf5 | #201a12 / #2a2318 |
| text / text-secondary | #33291f / #6f6150 | #f6f1e7 / #bcb3a6 |
| border | #ecdfc9 | #3a3225 |
| accent / on-accent | #eda100 / #231a06 | #f0b429 / #231a06 |
| accent-text (light のみ必須) | #7d5300 | (accent がリンク兼用可) |

**塗り専用アクセントの実例**: light の accent は対 bg 2:1 — CTA の塗りにだけ使い、リンク/リング/アイコンは
accent-text (#7d5300、6.2:1)。warning と hue 衝突 → warning は色面を使わずアイコン+ラベル形式。

### P5 夜光 (Dark Precision) — ダークが主テーマ + エレクトリックライム。開発者ツール / ダッシュボード。Liquid Glass 適性

| トークン | Dark (主) | Light |
| --- | --- | --- |
| bg / surface | #101210 / #181c17 | #fafaf7 / #ffffff |
| text / text-secondary | #f1f4ec / #adb6a6 | #191d16 / #5a6154 |
| border | #262b24 | #e3e6de |
| accent / on-accent | #c3ef4b / #1a2005 | #a5d823 / #1a2005 |
| accent-text (light のみ必須) | (accent がリンク兼用可・14.1:1) | #4a6b00 |

ユーザーが選んだ時点で「理由なきダーク」減点は免除。good と hue 近縁 → good は必ずチェックアイコン併用。

### P6 葡萄酒 (Bordeaux) — 深ワイン + ブラッシュ紙。ファッション / ビューティ / プレミアム

| トークン | Light | Dark |
| --- | --- | --- |
| bg / surface | #faf6f5 / #fffbfa | #1e1518 / #281d21 |
| text / text-secondary | #27181d / #6d5a60 | #f5eef0 / #bfb0b6 |
| border | #eadadd | #382b30 |
| accent / on-accent | #93304a / #ffffff | #e0879e / #2a0d14 |

## 9. 青が必要なとき

6 プリセットに青が無いのは意図 (AI 中央値からの離脱)。ブランド都合で青が要る場合の検証済みペア:
light `#245fa6` (対白 6.4:1、C 0.129) / dark `#6ea8e8` (対 #161414 7.4:1)。
H 265° を超えるインディゴ側へ寄せない — 「AI 紫」の入口。

## 10. 配分と検証

- **60-30-10**: bg/surface 60% / text・構造 30% / accent 10%。アクセントの使い所は CTA・リンク・
  フォーカスリング・主役数字。**登場回数が少なすぎて無彩色ページに見えるのは richness floor 減点** —
  ヒーローと主要 CTA には必ず色の力を入れる
- Phase 2 でトークンを書いたら即実行、Phase 4 でも再実行: `node scripts/validate-palette.mjs <tokens>`。
  プリセット改変時・納品後の色変更時も必ず通す
