# pptx-design.md — デザイン要件 + python-pptx フォールバック実装

目的: 「Office 既定テーマに画像を貼っただけ」に見えないマニュアルを、
コード生成で毎回同じ品質で出すこと。方針は ui-craft と同じ —
装飾で盛らず、階層・余白・一貫性でリッチに見せる。

## 0. デザイン要件 (生成エンジン非依存 — Track A / B 共通)

公式 pptx スキル (PptxGenJS) で生成する場合も、この要件は本スキル側の契約として必ず満たす:

1. **配色は「このアプリのための資料」に見せる。** ベースはほぼ無彩色 (紙色 + 墨色 + 罫線色)、
   アクセントはアプリの CSS から抽出したブランド色 1 色 + その淡色。汎用の青・虹色にしない。
   支配率をつける — 紙色が画面の大半、アクセントは番号丸・バー・章番号など「効かせる場所」だけ。
2. **視覚モチーフを 1 つ決めて全スライドで反復する** (例: タイトル脇のアクセント縦バー +
   番号丸コールアウト)。モチーフを途中で増やさない — 一貫性がリッチさの正体。
3. **表紙・章扉はコントラストで格を出す** (章扉は淡色パネルや大きな章番号で「区切り」を
   視覚化する)。本文スライドは白基調で画面キャプチャを主役にする —
   マニュアルの読者の目的は手順の追いやすさであり、営業資料的な演出は逆効果。
4. **全コンテンツスライドに視覚要素を置く** — このスキルでは原則スクショがそれを担う。
   スクショのない中間スライド (章扉・FAQ・用語集) はパターン化されたレイアウトで持たせる。
5. **スクショの縦横比は絶対に崩さない。** 枠線 1px で背景と分離する。
6. **コールアウトは boundingBox 座標から計算して置く編集可能シェイプ** (目分量・画像焼き込み禁止)。
7. **日本語フォントを全テキストに明示指定する** (既定フォント落ち禁止)。本文 14pt 前後・
   タイトル 28pt 前後・キャプション 10.5pt の 3〜4 段階に制限し、中間サイズを乱発しない。
8. **1 スライド 1 関心事。** 箇条書き 5 個・手順 6 個を超えたらスライドを分ける。

以下は Track B (python-pptx フォールバック) の実装詳細。

## 1. トークン定義 (スクリプト冒頭に必ず置く)

全レイアウト関数はこの定数だけを参照する。色を変えたければここだけ直せばよい構造にする。

```python
from pptx.util import Inches, Pt, Emu
from pptx.dml.color import RGBColor

# --- カラー (アクセントはアプリの CSS から抽出した色に差し替える) ---
INK        = RGBColor(0x1A, 0x1A, 0x1A)   # 本文
INK_SUB    = RGBColor(0x5F, 0x63, 0x68)   # 補足・キャプション
PAPER      = RGBColor(0xFF, 0xFF, 0xFF)   # 背景
PAPER_ALT  = RGBColor(0xF6, 0xF7, 0xF9)   # 交互背景・パネル
LINE       = RGBColor(0xE3, 0xE5, 0xE8)   # 罫線・画像枠
ACCENT     = RGBColor(0x0B, 0x57, 0xD0)   # ← アプリのブランド色で上書き
ACCENT_PALE= RGBColor(0xE8, 0xF0, 0xFE)   # ACCENT の淡色 (帯・強調背景)

# --- タイポグラフィ ---
FONT       = "Yu Gothic"        # Windows 標準。macOS 納品なら "Hiragino Sans" も可
SZ_TITLE   = Pt(28)             # スライドタイトル
SZ_H2      = Pt(18)             # 小見出し
SZ_BODY    = Pt(14)             # 本文・手順
SZ_CAPTION = Pt(10.5)           # キャプション・フッター

# --- レイアウトグリッド (16:9) ---
SLIDE_W, SLIDE_H = Inches(13.333), Inches(7.5)
MARGIN     = Inches(0.6)        # 全スライド共通の外周余白
TITLE_Y    = Inches(0.45)
CONTENT_Y  = Inches(1.35)       # タイトル帯の下端 = コンテンツ開始
GUTTER     = Inches(0.35)       # カラム間
```

## 2. 日本語フォントヘルパー (必須)

`run.font.name` は Latin フォントしか設定しない。East Asian (`a:ea`) を明示しないと
日本語が既定フォント (MS P ゴシック等) に落ち、環境によっては明朝・中華フォント化する。

```python
from pptx.oxml.ns import qn

def set_font(run, size=SZ_BODY, color=INK, bold=False, name=FONT):
    f = run.font
    f.size, f.bold, f.name = size, bold, name
    f.color.rgb = color
    rPr = run._r.get_or_add_rPr()
    for tag in ("a:latin", "a:ea"):
        el = rPr.find(qn(tag))
        if el is None:
            el = rPr.makeelement(qn(tag), {})
            rPr.append(el)
        el.set("typeface", name)
```

テキストを置くときは必ず `word_wrap = True`、余計な `auto_size` は使わない。
1 つのテキストフレームに複数段落を入れ、`space_after = Pt(6)` で行間を制御する。

## 3. 共通部品

### 背景とタイトル帯

既定レイアウトは使わない。毎スライド `slide_layouts[6]` (blank) から始め、自前で描く:

```python
def base_slide(prs, title=None, section=None):
    slide = prs.slides.add_slide(prs.slide_layouts[6])
    bg = slide.shapes.add_shape(MSO_SHAPE.RECTANGLE, 0, 0, SLIDE_W, SLIDE_H)
    bg.fill.solid(); bg.fill.fore_color.rgb = PAPER; bg.line.fill.background()
    bg.shadow.inherit = False
    if title:
        # アクセントの短い縦バー + タイトル (帯全塗りより上品)
        bar = slide.shapes.add_shape(MSO_SHAPE.RECTANGLE, MARGIN, TITLE_Y, Inches(0.07), Inches(0.5))
        bar.fill.solid(); bar.fill.fore_color.rgb = ACCENT; bar.line.fill.background()
        tb = slide.shapes.add_textbox(MARGIN + Inches(0.2), TITLE_Y - Inches(0.05), SLIDE_W - MARGIN*2, Inches(0.6))
        set_font(tb.text_frame.paragraphs[0].add_run_with(title), SZ_TITLE, INK, bold=True)  # 擬似コード: run を作って set_font
    return slide
```

全シェイプで `shadow.inherit = False` を設定する (既定の継承影が安っぽさの元)。

### フッター (表紙以外の全スライド)

左下にマニュアル名 (SZ_CAPTION, INK_SUB)、右下にページ番号。位置・サイズは全スライドで同一。

### スクリーンショットの配置

**縦横比を絶対に崩さない。** 枠に収める計算を関数化する:

```python
from PIL import Image

def place_screenshot(slide, path, box_x, box_y, box_w, box_h):
    iw, ih = Image.open(path).size
    scale = min(box_w / iw, box_h / ih)
    w, h = int(iw * scale), int(ih * scale)
    x = box_x + (box_w - w) // 2
    y = box_y + (box_h - h) // 2
    pic = slide.shapes.add_picture(path, x, y, w, h)
    pic.line.color.rgb = LINE; pic.line.width = Pt(1)   # 1px 枠 — 白背景と画面の境界を出す
    pic.shadow.inherit = False
    return pic  # コールアウト座標変換に配置矩形 (x, y, w, h) を使う
```

(PIL が無ければ `pptx.util` だけで `add_picture(path, x, y, width=...)` の片側指定でも可 —
その場合も必ず片側のみ指定して比率を守る。)

### コールアウト (番号丸)

Phase 2 で記録した boundingBox (viewport 1440×900 基準) を、画像の配置矩形に比例変換して置く:

```python
def add_callout(slide, n, bx, by, bw, bh, pic_x, pic_y, pic_w, pic_h):
    cx = pic_x + int((bx + bw / 2) / 1440 * pic_w)
    cy = pic_y + int((by + bh / 2) / 900 * pic_h)
    D = Inches(0.30)
    c = slide.shapes.add_shape(MSO_SHAPE.OVAL, cx - D // 2, cy - D // 2, D, D)
    c.fill.solid(); c.fill.fore_color.rgb = ACCENT
    c.line.color.rgb = PAPER; c.line.width = Pt(1.5)    # 白縁 — どんな背景でも視認できる
    c.shadow.inherit = False
    run = c.text_frame.paragraphs[0].add_run(); run.text = str(n)
    set_font(run, Pt(13), PAPER, bold=True)
```

- 番号はスライド右側の手順リストの番号と 1:1 対応させる。
- 丸が要素を隠すなら要素の右上角に寄せる。目分量で置かず必ず座標変換で置く。

## 4. スライドレイアウトパターン (この 7 種だけを使う)

| # | パターン | 構成 |
| --- | --- | --- |
| 1 | **表紙** | 上 2/3 余白 + 製品名 (SZ_CAPTION, ACCENT) / マニュアル題 (Pt36, bold) / 版・日付 (INK_SUB)。下端に ACCENT の細帯 (高さ 0.12in)。ロゴがあれば右上 |
| 2 | **目次** | 2 カラム。章番号を ACCENT・太字、章題を INK。ページ番号を右揃え |
| 3 | **章扉** | PAPER_ALT 全面背景 + 左に大きな章番号 (Pt80, ACCENT_PALE の上に ACCENT) + 章題 + この章でできることを 2〜3 行 |
| 4 | **画面概要** | 左 55%: スクショ (place_screenshot)。右 45%: 画面名 (H2) + 「この画面でできること」箇条書き 3〜5 個 + 「この画面の開き方」1 行 |
| 5 | **操作手順** | 左 60%: スクショ + コールアウト。右 40%: 番号付き手順 (①操作 → 結果)。1 スライド 1 タスク・手順は最大 6 個 — 超えるならスライドを分ける |
| 6 | **表** (FAQ・用語集) | ヘッダ行のみ ACCENT_PALE 背景 + 太字。罫線は水平のみ LINE 色 (格子にしない)。1 スライド最大 7 行 |
| 7 | **画面マップ** | 章ごとにグルーピングした画面名一覧 (サムネイルは任意)。全画面がどの章にあるかの索引 |

## 5. アンチテンプレ・チェックリスト (生成後に機械的に確認)

- [ ] Office 既定テーマ色 (既定の青 #4472C4・オレンジ) を使っていない — トークンの色だけ
- [ ] アクセントは 1 色 (+淡色) だけ。虹色・グラデーション・WordArt がない
- [ ] クリップアート・絵文字アイコンがない (アイコンが要る場面は「①」等の番号丸で足りる)
- [ ] 全スライドの余白 (MARGIN)・タイトル位置・フッター位置が揃っている
- [ ] スクショの縦横比が全て正しい (引き伸ばしゼロ)
- [ ] 箇条書きは 1 スライド 5 個以内・手順は 6 個以内 (超過はスライド分割で解決)
- [ ] 日本語が全て指定フォントで描画されている (`a:ea` 設定漏れがない)
- [ ] シェイプに既定の影が残っていない (`shadow.inherit = False` 漏れ)
- [ ] 文字あふれがない (テキストフレームの見積り: SZ_BODY で 1 行 ≒ 幅 1in あたり全角 4.5 字)
