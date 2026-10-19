import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";
import { webkit } from "playwright";

// Isolated, headless WebKit fixture: never connects to or controls SOURCE.
// Run after npm run build; install this test runtime with npx playwright install webkit.
const root = fileURLToPath(new URL("../", import.meta.url));
const server = await createServer({
  configFile: false, root, appType: "custom", logLevel: "error",
  optimizeDeps: { noDiscovery: true, entries: [] },
  resolve: { alias: { "@": `${root}src` } },
  server: { middlewareMode: true, hmr: false, ws: false, watch: null },
});
let browser;
try {
  const load = (name) => server.ssrLoadModule(`/src/components/ui/${name}.tsx`);
  const [{ Button }, { Switch }, { Slider }, { AnimatedTabNav }] = await Promise.all([
    load("button"), load("switch"), load("slider"), load("animated-tab-nav"),
  ]);
  const h = React.createElement;
  const controls = renderToStaticMarkup(h("div", { className: "app-ui cursor-default" },
    h(Button, { id: "button" }, h("span", { id: "button-label", className: "cursor-default" }, "Copy")),
    h(Button, { id: "disabled-button", disabled: true }, "Disabled"),
    h(Switch, { id: "switch", checked: true, onCheckedChange() {} }),
    h(Switch, { id: "disabled-switch", disabled: true }),
    h(Slider, { id: "slider", defaultValue: [50] }),
    h(Slider, { id: "disabled-slider", defaultValue: [50], disabled: true }),
    h(AnimatedTabNav, { tabs: [{ value: "timeline", label: "Timeline" }], value: "timeline", onValueChange() {} }),
  ));
  const roles = ["button", "tab", "switch", "checkbox", "radio", "option", "menuitem",
    "menuitemradio", "menuitemcheckbox", "combobox", "slider"];
  const custom = roles.map((role) => `
    <div id="role-${role}" role="${role}" class="cursor-default"><span id="child-${role}">${role}</span></div>
    <div id="disabled-${role}" role="${role}" aria-disabled="true" class="cursor-pointer"><span>Disabled</span></div>
  `).join("");
  const css = readdirSync(`${root}dist/assets`).filter((name) => name.endsWith(".css"))
    .map((name) => readFileSync(`${root}dist/assets/${name}`, "utf8")).join("\n");
  browser = await webkit.launch({ headless: true });
  const page = await browser.newPage();
  await page.setContent(`<style>${css}</style>${controls}${custom}
    <a id="link" href="#">Link</a>
    <details><summary id="summary">Expand</summary>Details</details>
    <label id="label" for="checkbox">Checkbox</label><input id="checkbox" type="checkbox">
    <input id="text"><textarea id="textarea"></textarea>
    <select id="select"><option>A</option></select>
    <div id="block" data-interactive="true" class="cursor-default">Block</div>
    <div id="select-scroll" data-slot="select-scroll-up-button" class="cursor-default">Up</div>
    <div class="cursor-pointer"><input id="nested-text" type="text">
      <div data-disabled><span id="disabled-child" class="cursor-pointer">Disabled slider part</span></div>
    </div>
    <div class="app-ui"><div data-slot="tabs-trigger" aria-disabled="true" id="disabled-tab-slot">Disabled tab</div></div>
    <div inert><button id="inert-button">Cannot interact</button></div>`);
  const check = async (selector, expected) => {
    const cursors = await page.locator(selector).evaluateAll((elements) =>
      elements.map((element) => getComputedStyle(element).cursor));
    assert.ok(cursors.length, `Fixture missing ${selector}`);
    for (const cursor of cursors) assert.equal(cursor, expected, selector);
  };
  for (const id of ["button", "button-label", "switch", "slider", "link", "summary",
    "label", "checkbox", "select", "block", "select-scroll"]) await check(`#${id}`, "pointer");
  await check("#slider [data-slot]", "pointer");
  await check("nav [role=tab]", "pointer");
  for (const role of roles) {
    await check(`#role-${role}`, "pointer");
    await check(`#child-${role}`, "pointer");
    await check(`#disabled-${role}`, "not-allowed");
    await check(`#disabled-${role} span`, "not-allowed");
  }
  for (const id of ["disabled-button", "disabled-switch", "disabled-slider", "disabled-child", "disabled-tab-slot", "inert-button"])
    await check(`#${id}`, "not-allowed");
  await check("#disabled-slider [data-slot]", "not-allowed");
  for (const id of ["text", "textarea", "nested-text"]) await check(`#${id}`, "text");
  console.log("Production CSS cursor checks passed in headless WebKit.");
} finally {
  await browser?.close();
  await server.close();
}
