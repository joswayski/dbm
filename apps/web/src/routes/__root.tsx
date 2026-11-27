/// <reference types="vite/client" />

import type { ReactNode } from "react";
import { createRootRoute, HeadContent, Outlet, Scripts } from "@tanstack/react-router";
import appCss from "../styles.css?url";
import redHatMono400 from "@fontsource/red-hat-mono/files/red-hat-mono-latin-400-normal.woff2?url";
import redHatMono700 from "@fontsource/red-hat-mono/files/red-hat-mono-latin-700-normal.woff2?url";
import { NotFound } from "../components/NotFound";
import { PRODUCT_NAME, SITE_DESCRIPTION, SITE_URL } from "../site";

export const Route = createRootRoute({
  head: () => ({
    meta: [
      { charSet: "utf-8" },
      { name: "viewport", content: "width=device-width, initial-scale=1" },
      { name: "theme-color", content: "#161618" },
      { title: `${PRODUCT_NAME} — database client for PostgreSQL, MySQL, and Redis` },
      { name: "description", content: SITE_DESCRIPTION },
      { property: "og:site_name", content: PRODUCT_NAME },
      { property: "og:type", content: "website" },
      { property: "og:url", content: SITE_URL },
      { property: "og:title", content: PRODUCT_NAME },
      { property: "og:description", content: SITE_DESCRIPTION },
      { property: "og:image", content: `${SITE_URL}/og.jpg` },
      { property: "og:image:width", content: "1200" },
      { property: "og:image:height", content: "750" },
      { property: "og:image:alt", content: "Anybase editing a PostgreSQL table" },
      { name: "twitter:card", content: "summary_large_image" },
      { name: "twitter:image", content: `${SITE_URL}/og.jpg` },
    ],
    links: [
      { rel: "stylesheet", href: appCss },
      // Fetch the UI font alongside the stylesheet instead of after it.
      ...[redHatMono400, redHatMono700].map((href) => ({
        rel: "preload",
        href,
        as: "font",
        type: "font/woff2",
        crossOrigin: "anonymous" as const,
      })),
      { rel: "icon", href: "/favicon.ico", sizes: "48x48" },
      { rel: "icon", href: "/favicon-32x32.png", type: "image/png", sizes: "32x32" },
      { rel: "apple-touch-icon", href: "/apple-touch-icon.png", sizes: "180x180" },
      { rel: "manifest", href: "/site.webmanifest" },
    ],
  }),
  component: RootDocument,
  notFoundComponent: NotFound,
});

function RootDocument() {
  return (
    <RootHtml>
      <Outlet />
    </RootHtml>
  );
}

function RootHtml({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <head>
        <HeadContent />
      </head>
      <body>
        {children}
        <Scripts />
      </body>
    </html>
  );
}
