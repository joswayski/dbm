// Regenerates docs/screenshots/*.png from the Vite browser preview in demo mode
// (`?demo`, see apps/desktop/ui/src/browserDemo.ts). No database or Tauri build
// is needed. Requires Playwright with Chromium:
//
//   npm install --global playwright && npx playwright install chromium
//   node scripts/screenshots.mjs
//
// Set DBM_CHROMIUM_PATH to use an existing Chromium binary instead.
import { execSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(ROOT, "docs/screenshots");
const VIEWPORT = { width: 1440, height: 900 };

const TOP_CUSTOMERS_SQL = `-- Top customers by paid revenue this year
SELECT u.full_name,
       u.plan,
       u.seats,
       count(i.number)      AS invoices,
       sum(i.amount)        AS paid_total,
       max(i.paid_at)::date AS last_paid
FROM users u
JOIN invoices i ON i.user_id = u.id
WHERE i.status = 'paid'
  AND i.paid_at >= date_trunc('year', now())
GROUP BY u.id
ORDER BY paid_total DESC
LIMIT 12;`;

async function loadPlaywright() {
  try {
    return await import("playwright");
  } catch {
    const globalRoot = execSync("npm root -g", { encoding: "utf8" }).trim();
    try {
      return createRequire(join(globalRoot, "noop.js"))("playwright");
    } catch {
      console.error("Playwright is not installed. Run: npm install --global playwright && npx playwright install chromium");
      process.exit(1);
    }
  }
}

const { chromium } = await loadPlaywright();
const server = await createServer({
  configFile: join(ROOT, "apps/desktop/ui/vite.config.ts"),
  root: join(ROOT, "apps/desktop/ui"),
  server: { port: 1431, strictPort: false },
  logLevel: "warn",
});
await server.listen();
const url = `${server.resolvedUrls.local[0]}?demo`;
const browser = await chromium.launch(process.env.DBM_CHROMIUM_PATH ? { executablePath: process.env.DBM_CHROMIUM_PATH } : {});

try {
  mkdirSync(OUT, { recursive: true });
  const page = await browser.newPage({ viewport: VIEWPORT, deviceScaleFactor: 2, colorScheme: "dark" });
  page.on("pageerror", (error) => { throw error; });
  const shot = async (name) => {
    await page.mouse.move(2, 2);
    await page.waitForTimeout(350);
    await page.screenshot({ path: join(OUT, name) });
    console.log(`docs/screenshots/${name}`);
  };
  const settle = () => page.waitForTimeout(350);

  await page.goto(url);
  await page.locator(".connection-main", { hasText: "Production" }).click();
  await page.getByRole("button", { name: "users", exact: true }).click();
  await page.locator("tbody tr").first().waitFor();

  const rows = page.locator("tbody tr");
  const editCell = async (row, column, text, commit = true) => {
    await rows.nth(row).locator("td").nth(column).dblclick();
    await page.keyboard.press("ControlOrMeta+A");
    await page.keyboard.type(text);
    if (commit) {
      await page.keyboard.press("Enter");
      await page.mouse.move(2, 2);
      await settle();
    }
  };

  // Staged edits, a staged delete, the inspector, and a cell mid-edit.
  await editCell(4, 2, "Ada Nguyen-Park");
  await editCell(11, 4, "8");
  await rows.nth(7).click({ button: "right" });
  await page.getByRole("button", { name: "Stage row for deletion" }).click();
  await page.mouse.move(2, 2);
  await settle();
  await rows.nth(2).click();
  await editCell(2, 3, "enterprise", false);
  // Focusing the editor can scroll the grid sideways; keep the key column in view.
  await page.locator(".tab-pane.active .grid-wrap").evaluate((grid) => { grid.scrollLeft = 0; });
  await shot("table-editing.png");

  // Hover diff for a staged edit.
  await page.keyboard.press("Escape");
  await rows.nth(4).hover();
  await settle();
  await page.screenshot({ path: join(OUT, "change-preview.png") });
  console.log("docs/screenshots/change-preview.png");

  // SQL workbench with results and history.
  await page.mouse.move(2, 2);
  await page.getByRole("button", { name: "Query 1", exact: true }).click();
  await page.getByRole("button", { name: "Rename Query 1" }).click();
  await page.getByRole("textbox", { name: "Rename Query 1" }).fill("top_customers.sql");
  await page.keyboard.press("Enter");
  await page.locator(".tab-pane.active .cm-content").click();
  await page.keyboard.press("ControlOrMeta+A");
  await page.keyboard.insertText(TOP_CUSTOMERS_SQL);
  await page.locator(".tab-pane.active .cm-content").click({ position: { x: 40, y: 60 } });
  await page.getByRole("button", { name: /Run statement/ }).click();
  await page.locator(".tab-pane.active .result-grid").waitFor();
  await shot("query.png");

  // Connection editor.
  await page.getByRole("button", { name: "Connection actions for Production" }).click();
  await page.getByRole("menuitem", { name: "Edit connection" }).click();
  await page.getByRole("dialog").waitFor();
  await shot("connection.png");
  await page.getByRole("button", { name: "Close", exact: true }).click();

  // A read-heavy table view without edits.
  await page.getByRole("button", { name: "invoices", exact: true }).click();
  await page.locator(".tab-pane.active tbody tr").first().waitFor();
  await page.locator(".tab-pane.active tbody tr").nth(5).click();
  await shot("table-browsing.png");
} finally {
  await browser.close();
  await server.close();
}
