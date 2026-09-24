import { writeFileSync } from "node:fs";
import { resolve } from "node:path";

// Takes the packaged app version from scripts/release.mjs, e.g. 2026.9.2401.
const [argument] = process.argv.slice(2);
const version = argument?.startsWith("v") ? argument.slice(1) : argument;
if (!/^\d+\.\d+\.\d+$/u.test(version ?? "")) {
  throw new Error("release version must use MAJOR.MINOR.PATCH");
}

const required = [
  "DBM_OFFICIAL_RELEASE",
  "TAURI_SIGNING_PRIVATE_KEY",
  "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
];
const missing = required.filter((name) => !process.env[name]);
if (missing.length > 0) {
  throw new Error(`missing release environment values: ${missing.join(", ")}`);
}

writeFileSync(
  resolve("apps/desktop/src-tauri/tauri.release.conf.json"),
  `${JSON.stringify({ version }, null, 2)}\n`,
);
