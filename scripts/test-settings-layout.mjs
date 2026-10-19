import assert from "node:assert/strict";
import { readFileSync, readdirSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";
import { webkit } from "playwright";

const root = fileURLToPath(new URL("../", import.meta.url));
const server = await createServer({
  configFile: false, root, appType: "custom", logLevel: "error",
  optimizeDeps: { noDiscovery: true, entries: [] }, resolve: { alias: { "@": `${root}src` } },
  server: { middlewareMode: true, hmr: false, ws: false, watch: null },
});
let browser;
try {
  const { SettingsLayout } = await server.ssrLoadModule("/src/components/settings/SettingsLayout.tsx");
  const { GeneralSettingsSection } = await server.ssrLoadModule("/src/components/settings/GeneralSettingsSection.tsx");
  const controller = { theme: "dark", showDescriptions: true, config: { auto_start: false },
    setTheme() {}, setShowDescriptions() {}, updateConfig() {}, resetToDefaults() {} };
  const render = (loading) => renderToStaticMarkup(React.createElement(SettingsLayout, {
    loading, saving: false, showDescriptions: true, tab: "general", onTabChange() {},
  }, React.createElement(GeneralSettingsSection, { controller })));
  const css = readdirSync(`${root}dist/assets`).filter((name) => name.endsWith(".css"))
    .map((name) => readFileSync(`${root}dist/assets/${name}`, "utf8")).join("\n");
  browser = await webkit.launch({ headless: true });
  const page = await browser.newPage();
  const setFixture = async (loading) => page.setContent(`<style>${css}</style>
    <div class="dark app-container" style="height:100vh;padding:24px">
      <div class="app-ui">${render(loading)}</div>
    </div>`);
  for (const width of [800, 1200, 1728]) {
    await page.setViewportSize({ width, height: 1000 });
    await setFixture(false);
    const bounds = await page.locator(".settings-page").boundingBox();
    assert.ok(Math.abs(bounds.x - (width - bounds.width) / 2) < 1, `Settings centered at ${width}px`);
    for (const selector of [".page-heading", ".settings-nav", ".settings-body", ".ui-panel"]) {
      const box = await page.locator(selector).first().boundingBox();
      assert.ok(Math.abs(box.x - bounds.x) < 1, `${selector} shares the column's left edge`);
      assert.ok(Math.abs(box.width - bounds.width) < 1, `${selector} shares the column's width`);
    }
  }
  mkdirSync("/tmp/source-settings-review", { recursive: true });
  await page.screenshot({ path: "/tmp/source-settings-review/centered-settings.png" });
  await setFixture(true);
  assert.equal(await page.locator('[role="status"]').textContent(), "Loading settings…");
  assert.equal(await page.locator(".settings-page").getAttribute("aria-busy"), "true");
  const spinner = page.locator('[role="status"] svg');
  assert.notEqual(await spinner.evaluate((el) => getComputedStyle(el).animationName), "none");
  await page.emulateMedia({ reducedMotion: "reduce" });
  assert.equal(await spinner.evaluate((el) => getComputedStyle(el).animationName), "none");
  const { CaptureChannelRow } = await server.ssrLoadModule("/src/components/settings/CaptureChannelRow.tsx");
  const { TooltipProvider } = await server.ssrLoadModule("/src/components/ui/tooltip.tsx");
  const unavailable = renderToStaticMarkup(React.createElement(TooltipProvider, {},
    React.createElement(CaptureChannelRow, {
      channel: { key: "keyboard", label: "Keyboard activity", description: "Keyboard capture", help: "Details" },
      controller: { ...controller, config: { capture_channels: { keyboard: true } },
        channelStatuses: [{ channel: "keyboard", health: "unavailable", supportsSoloTest: false,
          details: "Not available on macOS yet: this source's event listener is not implemented." }] },
    })));
  await page.setContent(`<style>${css}</style>${unavailable}`);
  assert.ok(await page.locator('[role="switch"]').isDisabled());
  assert.equal(await page.locator('[role="switch"]').getAttribute("aria-checked"), "false");
  assert.ok((await page.locator("body").textContent()).includes("Not available on macOS yet"));
  console.log("Centered Settings layout and loading animation checks passed in headless WebKit.");
} finally {
  await browser?.close();
  await server.close();
}
