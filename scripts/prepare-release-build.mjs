import { writeFileSync } from "node:fs";
import { resolve } from "node:path";

const [tag] = process.argv.slice(2);
const version = tag?.startsWith("v") ? tag.slice(1) : tag;
if (!/^\d+\.\d+\.\d+$/u.test(version ?? "")) {
  throw new Error("release tag must use vMAJOR.MINOR.PATCH");
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
