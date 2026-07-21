/* Dev-only quantitative rendered audit.
 *
 * Measures layout-coherence signals a reviewer would otherwise read from
 * pixels: element overlap, unreadable font sizes, clipped text, computed
 * WCAG contrast of key pairs, safety-bar geometry, and lane border grammar.
 * Prints a compact text report. Complements (never replaces) the screenshot
 * evidence and the assertion suite.
 */

import { chromium } from "@playwright/test";
import { startFixtureServer } from "./fixture-server.mjs";
import { LAUNCH_TOKEN } from "./fixtures.mjs";

const server = await startFixtureServer();
const browser = await chromium.launch();

function lum(c) {
  const [r, g, b] = c.map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(fg, bg) {
  const l1 = Math.max(lum(fg), lum(bg));
  const l2 = Math.min(lum(fg), lum(bg));
  return (l1 + 0.05) / (l2 + 0.05);
}

async function audit(vpName, vp, fixture, dark, actions) {
  await fetch(server.origin + "/__fixture", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ id: fixture }),
  });
  const ctx = await browser.newContext({
    viewport: vp,
    deviceScaleFactor: 2,
    colorScheme: dark ? "dark" : "light",
  });
  const page = await ctx.newPage();
  await page.goto(server.origin + "/");
  await page.fill("#token-input", LAUNCH_TOKEN);
  await page.click("#pair-button");
  await page.waitForSelector("#main .safety-bar, #main .terminal");
  if (actions) { await actions(page); }

  const report = await page.evaluate(() => {
    const out = { fonts: [], clipped: [], pairs: [], bar: null, lanes: [] };
    for (const el of document.querySelectorAll("#main *")) {
      const cs = getComputedStyle(el);
      if (el.textContent.trim() && el.children.length === 0) {
        const size = parseFloat(cs.fontSize);
        if (size < 11) { out.fonts.push(el.className + ":" + size.toFixed(1)); }
      }
      if ((cs.overflow === "hidden" || cs.overflowX === "hidden") &&
          el.scrollWidth > el.clientWidth + 2 && !el.classList.contains("visually-hidden")) {
        out.clipped.push(el.className + " " + el.scrollWidth + ">" + el.clientWidth);
      }
    }
    const rgb = (s) => (s.match(/\d+/g) || [255, 255, 255]).slice(0, 3).map(Number);
    const sample = (sel) => {
      const el = document.querySelector(sel);
      if (!el) { return null; }
      const cs = getComputedStyle(el);
      let bg = cs.backgroundColor;
      let node = el;
      while (bg === "rgba(0, 0, 0, 0)" && node.parentElement) {
        node = node.parentElement;
        bg = getComputedStyle(node).backgroundColor;
      }
      return { sel, fg: rgb(cs.color), bg: rgb(bg) };
    };
    for (const sel of [".capture-badge", ".badge-caution", ".bar-summary", ".event-row .row-meta",
      ".lane-state", ".lane-state.caution", ".limitation", ".pair-hint", "button"]) {
      const s = sample(sel);
      if (s) { out.pairs.push(s); }
    }
    const bar = document.querySelector(".safety-bar");
    if (bar) {
      const cs = getComputedStyle(bar);
      out.bar = { h: bar.offsetHeight, sticky: cs.position === "sticky", vh: window.innerHeight };
    }
    for (const lane of document.querySelectorAll(".lane")) {
      const cs = getComputedStyle(lane);
      out.lanes.push(lane.className.replace("lane ", "") + "=" + cs.borderTopStyle + "/" + cs.borderTopWidth);
    }
    out.docOverflow = (document.scrollingElement.scrollWidth - document.scrollingElement.clientWidth);
    return out;
  });

  console.log("== " + vpName + " " + fixture + (dark ? " dark" : ""));
  console.log("bar:", JSON.stringify(report.bar), "docOverflowX:", report.docOverflow);
  console.log("lanes:", report.lanes.join("  "));
  console.log("small-fonts(<11px):", report.fonts.length ? report.fonts.slice(0, 8).join(", ") : "none");
  console.log("clipped:", report.clipped.length ? report.clipped.slice(0, 8).join(", ") : "none");
  for (const p of report.pairs) {
    console.log("contrast " + p.sel + ": " + contrast(p.fg, p.bg).toFixed(2));
  }
  console.log("");
  await ctx.close();
}

const openDomains = async (page) => {
  const row = page.locator(".event-row").first();
  if (await row.count()) {
    await row.click();
    await page.click("#tab-domains");
  }
};

await audit("V2 1440x900", { width: 1440, height: 900 }, "fx-default", false, openDomains);
await audit("V2 dark", { width: 1440, height: 900 }, "fx-owner-both", true, openDomains);
await audit("V7 360x640", { width: 360, height: 640 }, "fx-default", false, openDomains);
await audit("V9 320x256", { width: 320, height: 256 }, "fx-owner-both", false, openDomains);

await browser.close();
await server.close();
