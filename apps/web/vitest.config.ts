import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    projects: [
      {
        test: {
          name: "web",
          globals: true,
          include: ["tests/**/*.test.ts"],
          isolate: true,
          restoreMocks: true,
          unstubGlobals: true,
        },
      },
      {
        test: {
          name: "release",
          root: fileURLToPath(new URL("../../", import.meta.url)),
          include: ["scripts/**/*.test.mjs"],
          isolate: true,
          restoreMocks: true,
          unstubGlobals: true,
        },
      },
    ],
  },
});
