import { createFileRoute } from "@tanstack/react-router";
import { DownloadRedirect } from "../components/DownloadRedirect";
import { PRODUCT_NAME, SITE_URL } from "../site";

/** anyba.se/download: one link that opens the right install on any device. */
export const Route = createFileRoute("/download")({
  head: () => ({
    meta: [
      { title: `Download — ${PRODUCT_NAME}` },
      { name: "robots", content: "noindex" },
      { property: "og:url", content: `${SITE_URL}/download` },
    ],
  }),
  component: DownloadRedirect,
});
