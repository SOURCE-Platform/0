import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";

const root = fileURLToPath(new URL("../", import.meta.url));
const server = await createServer({
  configFile: false, root, appType: "custom", logLevel: "error",
  optimizeDeps: { noDiscovery: true, entries: [] },
  resolve: { alias: { "@": `${root}src` } },
  server: { middlewareMode: true, hmr: false, ws: false, watch: null },
});
try {
  const { formatMoney } = await server.ssrLoadModule("/src/components/banking/format.ts");
  assert.equal(formatMoney({ amount: "9007199254740993.01", currency: "PLN" }).replaceAll(/\s/g, ""),
    "9007199254740993,01PLN");
  assert.equal(formatMoney({ amount: "-0.01", currency: "EUR" }), "-0,01 EUR");
  assert.equal(formatMoney({ amount: "1e3", currency: "PLN" }), "Unavailable PLN");
  const { BankAccounts } = await server.ssrLoadModule("/src/components/banking/BankAccounts.tsx");
  const accounts = [{
    id: "synthetic-account", label: "Personal", currency: "PLN", active: true, maskedNumber: "•••• 0001",
    balances: [
      { money: { amount: "1200.01", currency: "PLN" }, balanceType: "CLBD", observedAt: "2026-10-18T12:00:00Z" },
      { money: { amount: "100.00", currency: "EUR" }, balanceType: "ITAV", observedAt: "2026-10-18T12:00:00Z" },
    ],
  }];
  const cards = renderToStaticMarkup(React.createElement(BankAccounts, { accounts }));
  assert.ok(cards.includes("PLN") && cards.includes("EUR") && !cards.includes("$"));
  assert.ok(cards.includes("Booked balance") && cards.includes("Available balance"));
  assert.ok(cards.includes("not proof of complete history"));
  const { BankTransactions } = await server.ssrLoadModule("/src/components/banking/BankTransactions.tsx");
  const html = renderToStaticMarkup(React.createElement(BankTransactions, {
    accounts, query: { offset: 0, limit: 50 }, busy: false, onFilter() {},
    page: { total: 1, rows: [{
      id: "synthetic-tx", accountId: "synthetic-account", money: { amount: "12.34", currency: "PLN" },
      direction: "outgoing", status: "PDNG", date: null, description: "<script>synthetic</script>",
      counterparty: null, identityQuality: "unverified-fingerprint", revisionCount: 1,
    }] },
  }));
  assert.ok(html.includes("−12,34 PLN") && html.includes("Pending"));
  assert.ok(html.includes("Matching is provisional"));
  assert.ok(html.includes("&lt;script&gt;synthetic&lt;/script&gt;") && !html.includes("<script>"));
  assert.ok(html.includes('aria-label="Filter by account"') && html.includes('aria-label="Transactions from"'));
  assert.equal((html.match(/disabled=""/g) ?? []).length, 2);
  const { BankingSetup } = await server.ssrLoadModule("/src/components/banking/BankingSetup.tsx");
  const setup = renderToStaticMarkup(React.createElement(BankingSetup, { configured: false, busy: false, onConfigure() {} }));
  assert.ok(setup.includes("https://sourceovcourse.com/banking/callback"));
  assert.ok(setup.includes("https://sourceovcourse.com/privacy/banking"));
  assert.ok(setup.includes("https://sourceovcourse.com/privacy/terms"));
  assert.ok(setup.includes("application-id") && !setup.includes('type="password"'));
  assert.ok(setup.includes('href="https://enablebanking.com/sign-in/"'));
  assert.ok(setup.includes('href="https://enablebanking.com/cp/applications"'));
  assert.ok(setup.includes("Activate by linking accounts") && setup.includes("Production"));
  const { SettingsLayout } = await server.ssrLoadModule("/src/components/settings/SettingsLayout.tsx");
  const settings = renderToStaticMarkup(React.createElement(SettingsLayout, {
    loading: false, saving: false, showDescriptions: true, tab: "banking", onTabChange() {},
  }, React.createElement("div", {}, "Banking settings content")));
  assert.ok(settings.includes(">Banking<span") && settings.includes('data-section="banking"'));
  assert.ok(!settings.includes("Changes save automatically"));
  const { default: BankingPage } = await server.ssrLoadModule("/src/components/banking/BankingPage.tsx");
  const embedded = renderToStaticMarkup(React.createElement(BankingPage, { embedded: true }));
  assert.ok(!embedded.includes("<h1") && embedded.includes("Personal-use preview"));
  console.log("Banking UI checks passed: exact amounts, currencies, provenance warnings, escaping, filters, and setup.");
} finally {
  await server.close();
}
