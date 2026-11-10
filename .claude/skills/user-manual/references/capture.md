# capture.md — ルート列挙と Playwright 撮影レシピ

## 1. ルート列挙 (フレームワーク別)

コードのルート定義が唯一の情報源。README の画面一覧は参考情報であり根拠にしない。

| フレームワーク | 検出の手がかり | 列挙方法 |
| --- | --- | --- |
| Next.js (App Router) | `app/**/page.{tsx,jsx,js}` | glob → パスに変換。route group `(name)` は除去、`[param]` は動的ルート扱い |
| Next.js (Pages Router) | `pages/**/*.{tsx,jsx,js}` | glob。`_app` `_document` `api/` は除外 |
| React Router | `createBrowserRouter` / `<Route path=` | grep してルート配列・JSX から path を抽出 |
| Vue Router | `router/index.{ts,js}` の routes 配列 | routes 配列を読む。Nuxt は `pages/**` glob |
| SvelteKit | `src/routes/**/+page.svelte` | glob。`(group)` 除去 |
| Angular | `*-routing.module.ts` / `provideRouter` | Routes 配列を読む (lazy load の子ルートも辿る) |
| Rails | `config/routes.rb` | `bin/rails routes` を実行して GET の HTML ルートを抽出 |
| Django | `urls.py` | urlpatterns を読む (include を再帰的に辿る) |
| Laravel | `routes/web.php` | `php artisan route:list --method=GET` |
| SPA (定義が追えない) | — | 最終手段: `/` から同一オリジンのリンクを辿るクロール。**クロールで列挙した旨を報告に明記** (網羅保証が弱い) |

### routes.json (ルートレジストリ) スキーマ

```json
[
  {
    "path": "/orders/[id]",
    "url": "http://localhost:3000/orders/42",
    "name": "注文詳細",
    "group": "注文管理",
    "requiresAuth": true,
    "excluded": false,
    "excludeReason": null,
    "verified": null
  }
]
```

- `group` はマニュアルの章に対応させる (ナビゲーション構造・URL 階層から決める)。
- 動的ルートは代表データの実 URL を `url` に入れる (seed / fixture / DB から探す)。
- 対象外 (excluded) にできるのは: API ルート、開発用ページ (`/dev`, `/storybook`)、
  エラーページ (404 等は「トラブル時の画面」として載せる価値があれば含める)。理由必須。

## 2. 環境準備

```bash
npx playwright --version || npm i -D playwright   # プロジェクトに合わせ pnpm/yarn/bun
npx playwright install chromium                    # ブラウザ未取得なら
```

dev server は既存プロセスを優先 (`curl -sf http://localhost:PORT` で確認)。
自分で起動する場合は run_in_background で起動し、ready を待ってから撮影、**終了後に必ず止める**。

## 3. 撮影スクリプトの骨格

ワークスペースに `capture.mjs` として保存する (差分更新で再利用)。

```js
import { chromium } from 'playwright';
import fs from 'node:fs';

const routes = JSON.parse(fs.readFileSync('routes.json', 'utf8'));
const browser = await chromium.launch();
const ctx = await browser.newContext({
  viewport: { width: 1440, height: 900 },
  deviceScaleFactor: 2,          // Retina 画質 — スライドに載せても粗くならない
  locale: 'ja-JP',
  reducedMotion: 'reduce',
  storageState: fs.existsSync('auth.json') ? 'auth.json' : undefined,
});
const page = await ctx.newPage();

for (const r of routes.filter(r => !r.excluded)) {
  await page.goto(r.url, { waitUntil: 'networkidle' });
  await page.evaluate(() => document.fonts.ready);
  // アニメーション・カーソル点滅を止める (ブレ・撮影ごとの差分を防ぐ)
  await page.addStyleTag({ content:
    '*,*::before,*::after{animation:none!important;transition:none!important;caret-color:transparent!important}' });
  const slug = r.path.replaceAll('/', '_').replace(/[\[\]]/g, '') || 'home';
  await page.screenshot({ path: `shots/${slug}.png` });                    // スライド掲載用
  await page.screenshot({ path: `shots/${slug}.full.png`, fullPage: true }); // 参考用
  // 主要 UI 要素の boundingBox を記録 → コールアウト配置に使う
  const boxes = await page.evaluate(() =>
    [...document.querySelectorAll('button, a[href], input, select, textarea, [role="button"]')]
      .filter(el => el.checkVisibility() && el.getBoundingClientRect().width > 0)
      .map(el => {
        const b = el.getBoundingClientRect();
        return { label: (el.innerText || el.getAttribute('aria-label') || el.name || '').trim().slice(0, 40),
                 tag: el.tagName.toLowerCase(), x: b.x, y: b.y, w: b.width, h: b.height };
      })
      .filter(b => b.y < 900));   // ビューポート内のみ
  fs.writeFileSync(`shots/${slug}.boxes.json`, JSON.stringify(boxes, null, 2));
}
await browser.close();
```

### 認証 (必要な場合)

ログインフローを 1 回だけ実行して保存し、以降は storageState で再利用:

```js
await page.goto(loginUrl);
await page.fill('input[name="email"]', process.env.MANUAL_LOGIN_EMAIL);
await page.fill('input[name="password"]', process.env.MANUAL_LOGIN_PASSWORD);
await page.click('button[type="submit"]');
await page.waitForURL(u => !u.pathname.includes('login'));
await ctx.storageState({ path: 'auth.json' });
```

- 認証情報は環境変数か実行時入力で受け取る。**スクリプトに直書きしない。**
- `auth.json` はセッショントークンを含む — ワークスペース内に置き `.git/info/exclude` 対象。

### 状態ショット (モーダル・タブ・エラー表示)

Phase 3 で必要になったら、操作を再現してから撮る:

```js
await page.click('text=新規作成');
await page.waitForSelector('[role="dialog"]');
await page.screenshot({ path: 'shots/orders_new_modal.png' });
```

### 実データのマスク

staging 等で実データが写る場合は locator 単位でマスクできる:

```js
await page.screenshot({ path, mask: [page.locator('.customer-name')], maskColor: '#E8EAED' });
```

### 揺らぐ表示の固定 (任意)

相対時刻 (「3 分前」) やランダム表示で差分更新のたびに画像が変わるのが問題になる場合のみ、
`page.clock.setFixedTime(...)` で時刻を固定する。

## 4. 目視検証チェックリスト (全数・Read で画像を開いて)

1 枚ずつ以下を確認し、レジストリの `verified` に結果と理由を記録する:

- [ ] 白画面・スケルトン・スピナーのまま写っていないか (待機不足 → wait を追加して撮り直し)
- [ ] ログイン画面へリダイレクトされていないか (認証切れ → storageState を作り直し)
- [ ] 404 / 500 / エラーオーバーレイ (Next.js の赤枠等) が写っていないか
- [ ] `undefined` / `NaN` / 翻訳キー生 (`common.title`) が表示に出ていないか —
      **これはアプリ側のバグの可能性がある。撮り直しではなくユーザーへの報告対象**
- [ ] データが空すぎて画面の説明にならなくないか (空一覧 → seed 投入を検討)
- [ ] 実在の個人情報・トークン・内部ホスト名が写っていないか
- [ ] 日本語フォントが正しく描画されているか (豆腐・中華フォント化していないか)
