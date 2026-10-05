import { tanstackStart } from "@tanstack/react-start/plugin/vite";
import tailwindcss from "@tailwindcss/vite";
import viteReact from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { fetchLatestChanges, type LatestChange } from "./src/latestChanges.ts";
import { REPOSITORY } from "./src/site.ts";

type BuildData = { latestChanges: LatestChange[]; builtAt: number };

/**
 * One `vite build` loads this config several times (client, server, prerender).
 * Fetch once per process so every bundle embeds the same list and timestamp.
 */
const buildCache = globalThis as typeof globalThis & { __anybaseBuildData?: Promise<BuildData> };

async function loadBuildData(): Promise<BuildData> {
  const latestChanges = await fetchLatestChanges(REPOSITORY, process.env.GITHUB_TOKEN?.trim());
  console.log(`Fetched ${latestChanges.length} latest changes from the GitHub API.`);
  return { latestChanges, builtAt: Date.now() };
}

export default defineConfig(async () => {
  buildCache.__anybaseBuildData ??= loadBuildData();
  const { latestChanges, builtAt } = await buildCache.__anybaseBuildData;

  return {
    plugins: [
      tanstackStart({
        prerender: {
          enabled: true,
          autoStaticPathsDiscovery: true,
          crawlLinks: false,
          failOnError: true,
        },
        pages: [
          {
            path: "/404",
            sitemap: { exclude: true },
            prerender: { crawlLinks: false, outputPath: "/404.html" },
          },
        ],
      }),
      viteReact(),
      tailwindcss(),
    ],
    define: {
      __LATEST_CHANGES__: JSON.stringify(latestChanges),
      // Server and client render relative times from the same instant, then the
      // page switches to the visitor's clock after hydration.
      __BUILT_AT__: JSON.stringify(builtAt),
    },
    server: {
      port: 5175,
    },
  };
});
