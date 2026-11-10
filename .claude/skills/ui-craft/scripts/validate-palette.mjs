#!/usr/bin/env node
// validate-palette.mjs — ui-craft カラートークンの機械検証 (依存ゼロ / Node 18+)
//
// 用途:
//   1. references/color-system.md のプリセット事前検証 (リポジトリ側)
//   2. Phase 2 (トークン定義直後) / Phase 4 (納品前ゲート) での実装トークン検証
//   3. 納品後にユーザーが色を変えたときの再検証 (微調整ガイドに記載)
//
// 使い方:
//   node validate-palette.mjs tokens.css       # :root=light / dark ブロック自動検出
//   node validate-palette.mjs tokens.json      # {"light": {"bg": "#..", ...}, "dark": {...}}
//   node validate-palette.mjs --pair "#7d5300:#fbf5ea:4.5:リンク色" [--pair ...]
//   node validate-palette.mjs --self-test
//   オプション: --json (機械可読出力)
//
// 閾値の根拠は references/color-system.md §コントラスト基準 が正。
// 半透明 (8 桁 hex / rgba / color-mix) は検証不能 — 合成後の実効色で検証すること。
// チャート配色の CVD ペア検証 (Machado 行列) は dataviz スキルの責務のため対象外。

import { readFileSync } from "node:fs";
import process from "node:process";

const KNOWN_KEYS = [
  "bg", "surface", "surface2", "text", "textSecondary", "border",
  "accent", "onAccent", "accentText",
];
const NEUTRAL_KEYS = ["bg", "surface", "surface2", "border", "text", "textSecondary"];
const HEX_RE = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i;

// ---- 色変換 -----------------------------------------------------------------

function hexToRgb(hex) {
  const m = HEX_RE.exec(hex.trim());
  if (!m) return null;
  let h = m[1];
  if (h.length === 3) h = [...h].map((c) => c + c).join("");
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
}

const srgbToLinear = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);

function relativeLuminance(hex) {
  const [r, g, b] = hexToRgb(hex).map(srgbToLinear);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function wcagRatio(hex1, hex2) {
  const [a, b] = [relativeLuminance(hex1), relativeLuminance(hex2)];
  const [hi, lo] = a >= b ? [a, b] : [b, a];
  return (hi + 0.05) / (lo + 0.05);
}

// sRGB → OKLCH (Björn Ottosson の OKLab 行列)
function srgbToOklch(hex) {
  const [r, g, b] = hexToRgb(hex).map(srgbToLinear);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  const C = Math.hypot(A, B);
  const H = ((Math.atan2(B, A) * 180) / Math.PI + 360) % 360;
  return { L, C, H };
}

// APCA-W3 0.1.9 相当 (Lc)。品質目標の参考値であり、機械ゲートは WCAG 側。
function apcaLc(txtHex, bgHex) {
  const y = (hex) => {
    const [r, g, b] = hexToRgb(hex);
    let Y = 0.2126729 * r ** 2.4 + 0.7151522 * g ** 2.4 + 0.072175 * b ** 2.4;
    if (Y < 0.022) Y += (0.022 - Y) ** 1.414; // soft black clamp
    return Y;
  };
  const [yTxt, yBg] = [y(txtHex), y(bgHex)];
  if (Math.abs(yBg - yTxt) < 0.0005) return 0;
  let sapc;
  if (yBg > yTxt) {
    sapc = (yBg ** 0.56 - yTxt ** 0.57) * 1.14; // 通常極性 (明地に暗文字)
    return sapc < 0.1 ? 0 : (sapc - 0.027) * 100;
  }
  sapc = (yBg ** 0.65 - yTxt ** 0.62) * 1.14; // 逆極性 (暗地に明文字)
  return sapc > -0.1 ? 0 : (sapc + 0.027) * 100;
}

// ---- 入力パース --------------------------------------------------------------

function varToKey(name) {
  const key = name
    .replace(/^--color-/, "")
    .replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase());
  return key === "surface2" || KNOWN_KEYS.includes(key) ? key : null;
}

// :root = light、@media (prefers-color-scheme: dark) / [data-theme="dark"] = dark
function parseCssThemes(css) {
  const themes = { light: {}, dark: {} };
  const skipped = [];
  const src = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const stack = [];
  let buf = "";
  for (const ch of src) {
    if (ch === "{") { stack.push(buf.trim()); buf = ""; continue; }
    if (ch === "}") { stack.pop(); buf = ""; continue; }
    if (ch !== ";") { buf += ch; continue; }
    const m = buf.trim().match(/^(--color-[a-z0-9-]+)\s*:\s*(.+)$/i);
    buf = "";
    if (!m) continue;
    const ctx = stack.join(" ");
    const theme = /prefers-color-scheme:\s*dark|data-theme=["']dark["']/.test(ctx) ? "dark" : "light";
    const key = varToKey(m[1]);
    const val = m[2].trim();
    if (!key) skipped.push(`${theme}: ${m[1]} (未知トークン — 検査対象外)`);
    else if (HEX_RE.test(val)) themes[theme][key] = val.toLowerCase();
    else skipped.push(`${theme}: ${m[1]}: ${val} (hex 以外は検証不能 — 実効色で --pair 検証を)`);
  }
  return { themes, skipped };
}

// ---- チェック本体 ------------------------------------------------------------

function buildChecks(theme, t) {
  const rows = [];
  const add = (check, pair, value, threshold, status, note = "") =>
    rows.push({ theme, check, pair, value, threshold, status, note });
  const ratio = (a, b) => Math.round(wcagRatio(t[a], t[b]) * 100) / 100;

  for (const req of ["bg", "text", "accent"]) {
    if (!t[req]) add("必須トークン", req, "-", "定義必須", "ERROR", `${req} が未定義`);
  }
  if (rows.some((r) => r.status === "ERROR")) return rows;

  const wcag = (label, a, b, min, status = "FAIL") => {
    if (!t[a] || !t[b]) return null;
    const v = ratio(a, b);
    add(label, `${a}/${b}`, `${v}:1`, `>= ${min}:1`, v >= min ? "PASS" : status);
    return v;
  };

  wcag("本文コントラスト", "text", "bg", 4.5);
  wcag("本文コントラスト", "text", "surface", 4.5);
  wcag("補助テキスト", "textSecondary", "bg", 4.5);
  if (t.onAccent) wcag("塗り CTA の文字", "onAccent", "accent", 4.5);
  else add("塗り CTA の文字", "onAccent/accent", "-", ">= 4.5:1", "WARN", "塗り CTA を作るなら on-accent を定義");

  // accent/bg: accentText があれば accent は塗り専用として 3:1 未満を WARN に緩和
  // (その場合リンク・フォーカスリング・アイコンは accentText を使う契約)
  const accentBg = ratio("accent", "bg");
  if (t.accentText) {
    add("アクセント対地", "accent/bg", `${accentBg}:1`, ">= 3:1", accentBg >= 3 ? "PASS" : "WARN",
      accentBg < 3 ? "塗り専用 — リンク/リング/アイコンは accent-text を使う" : "");
    wcag("リンク色", "accentText", "bg", 4.5);
  } else {
    add("アクセント対地 (UI 部品)", "accent/bg", `${accentBg}:1`, ">= 3:1", accentBg >= 3 ? "PASS" : "FAIL");
    if (accentBg < 4.5)
      add("リンク色", "accent/bg", `${accentBg}:1`, ">= 4.5:1", "WARN", "リンクに使うなら accent-text を定義");
  }

  const ac = srgbToOklch(t.accent);
  add("アクセント彩度", "accent", `C ${ac.C.toFixed(3)}`, "C >= 0.10", ac.C >= 0.1 ? "PASS" : "FAIL",
    ac.C < 0.1 ? "彩度不足 — グレーに見えて identity を失う" : "");
  if (ac.L < 0.4 || ac.L > 0.9)
    add("アクセント明度", "accent", `L ${ac.L.toFixed(3)}`, "L 0.40-0.90", "WARN", "役割別 L バンド (color-system.md §3) を確認");

  for (const k of NEUTRAL_KEYS) {
    if (!t[k]) continue;
    const { C } = srgbToOklch(t[k]);
    if (C > 0.05) add("ニュートラル彩度", k, `C ${C.toFixed(3)}`, "C <= 0.05", "WARN", "彩度過多 — ニュートラルの役割を失う");
    else if (C < 0.001) add("ニュートラル彩度", k, "C 0.000", "hue バイアス推奨", "INFO", "純グレーは無考慮に見える (C 0.002-0.03 を推奨)");
  }

  const apca = (label, a, b, min) => {
    if (!t[a] || !t[b]) return;
    const lc = Math.abs(apcaLc(t[a], t[b]));
    add(label, `${a}/${b}`, `Lc ${lc.toFixed(1)}`, `Lc >= ${min}`, lc >= min ? "PASS" : "WARN",
      lc < min ? "APCA は品質目標 (WCAG ゲートとは独立)" : "");
  };
  apca("APCA 本文", "text", "bg", 75);
  apca("APCA 補助テキスト", "textSecondary", "bg", 60);

  return rows;
}

// ---- 出力 --------------------------------------------------------------------

function report(rows, skipped, asJson) {
  const count = (s) => rows.filter((r) => r.status === s).length;
  const summary = { PASS: count("PASS"), WARN: count("WARN"), INFO: count("INFO"), FAIL: count("FAIL"), ERROR: count("ERROR") };
  if (asJson) {
    console.log(JSON.stringify({ rows, skipped, summary }, null, 2));
  } else {
    const w = (s, n) => String(s).padEnd(n);
    for (const r of rows) {
      const line = `${w(r.status, 6)} ${w(r.theme, 6)} ${w(r.check, 14)} ${w(r.pair, 26)} ${w(r.value, 12)} ${w(r.threshold, 12)} ${r.note}`;
      console.log(line.trimEnd());
    }
    for (const s of skipped) console.log(`SKIP   ${s}`);
    console.log(`\nsummary: PASS ${summary.PASS} / WARN ${summary.WARN} / INFO ${summary.INFO} / FAIL ${summary.FAIL} / ERROR ${summary.ERROR}`);
  }
  return summary.FAIL + summary.ERROR > 0 ? 1 : 0;
}

// ---- self-test ---------------------------------------------------------------

function selfTest() {
  const near = (a, b, eps) => Math.abs(a - b) <= eps;
  const results = [];
  const t = (name, ok) => results.push({ name, ok });

  t("WCAG 白/黒 = 21:1", near(wcagRatio("#ffffff", "#000000"), 21, 0.01));
  t("APCA #888/#fff ≈ 63.06", near(apcaLc("#888888", "#ffffff"), 63.06, 0.5));
  t("APCA #fff/#000 ≈ -107.88", near(apcaLc("#ffffff", "#000000"), -107.88, 0.5));
  const red = srgbToOklch("#ff0000");
  t("OKLCH red (L .628 / C .258 / H 29.2)", near(red.L, 0.628, 0.002) && near(red.C, 0.2577, 0.002) && near(red.H, 29.23, 0.3));
  t("OKLCH white (L 1.0 / C 0)", near(srgbToOklch("#ffffff").L, 1, 0.001) && srgbToOklch("#ffffff").C < 0.001);

  // フィクスチャ: 墨と朱 light (正は references/color-system.md §プリセット)
  const monoEdgeLight = {
    bg: "#fdfcfb", surface: "#ffffff", text: "#1a1516", textSecondary: "#5f5a58",
    border: "#e8e6e4", accent: "#d92d0c", onAccent: "#ffffff",
  };
  const okRows = buildChecks("light", monoEdgeLight);
  t("墨と朱 light に FAIL なし", okRows.every((r) => r.status !== "FAIL" && r.status !== "ERROR"));

  const badRows = buildChecks("light", { bg: "#ffffff", text: "#aaaaaa", accent: "#ec835a", onAccent: "#ffffff" });
  t("故意の失敗例で FAIL を検出", badRows.some((r) => r.status === "FAIL"));

  const css = `:root { --color-bg: #fdfcfb; --color-text: #1a1516; }
    @media (prefers-color-scheme: dark) { :root { --color-bg: #161414; } }
    [data-theme="dark"] { --color-text: #f4f1f0; --color-accent: var(--brand); }`;
  const { themes, skipped } = parseCssThemes(css);
  t("CSS パース: light/dark を分離", themes.light.bg === "#fdfcfb" && themes.dark.bg === "#161414" && themes.dark.text === "#f4f1f0");
  t("CSS パース: var() を SKIP", skipped.length === 1);

  for (const r of results) console.log(`${r.ok ? "PASS" : "FAIL"}  ${r.name}`);
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\nself-test: ${results.length - failed}/${results.length} passed`);
  return failed > 0 ? 1 : 0;
}

// ---- main --------------------------------------------------------------------

function main() {
  const argv = process.argv.slice(2);
  const asJson = argv.includes("--json");
  if (argv.includes("--self-test")) process.exit(selfTest());

  const rows = [];
  const skipped = [];

  const pairs = argv.flatMap((a, i) => (a === "--pair" && argv[i + 1] ? [argv[i + 1]] : []));
  for (const p of pairs) {
    const [fg, bg, min = "4.5", label = "pair"] = p.split(":");
    if (!hexToRgb(fg) || !hexToRgb(bg)) {
      rows.push({ theme: "-", check: "pair", pair: p, value: "-", threshold: "-", status: "ERROR", note: "hex を解釈できない" });
      continue;
    }
    const v = Math.round(wcagRatio(fg, bg) * 100) / 100;
    rows.push({
      theme: "-", check: label, pair: `${fg}/${bg}`, value: `${v}:1`, threshold: `>= ${min}:1`,
      status: v >= parseFloat(min) ? "PASS" : "FAIL", note: "",
    });
  }

  const file = argv.find((a) => !a.startsWith("--") && a !== pairs.find((p) => p === a));
  if (file) {
    let themes;
    const text = readFileSync(file, "utf8");
    if (file.endsWith(".json")) {
      const data = JSON.parse(text);
      themes = { light: data.light ?? {}, dark: data.dark ?? {} };
      for (const [theme, tokens] of Object.entries(themes)) {
        for (const [k, v] of Object.entries(tokens)) {
          if (!KNOWN_KEYS.includes(k)) { skipped.push(`${theme}: ${k} (未知トークン)`); delete tokens[k]; }
          else if (!HEX_RE.test(String(v))) { skipped.push(`${theme}: ${k}: ${v} (hex 以外 — 8 桁 hex/半透明は実効色で)`); delete tokens[k]; }
        }
      }
    } else {
      const parsed = parseCssThemes(text);
      themes = parsed.themes;
      skipped.push(...parsed.skipped);
    }
    for (const [theme, tokens] of Object.entries(themes)) {
      if (Object.keys(tokens).length > 0) rows.push(...buildChecks(theme, tokens));
    }
    if (rows.length === 0) {
      console.error("トークンが 1 つも見つからない (--color-* が定義されているか確認)");
      process.exit(1);
    }
  }

  if (rows.length === 0) {
    console.error("使い方: node validate-palette.mjs <tokens.css|tokens.json> | --pair \"#fg:#bg:4.5:label\" | --self-test");
    process.exit(1);
  }
  process.exit(report(rows, skipped, asJson));
}

main();
