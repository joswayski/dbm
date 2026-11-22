import { createFileRoute } from "@tanstack/react-router";
import { DownloadRedirect } from "../components/DownloadRedirect";
import { PRODUCT_NAME, SITE_URL } from "../site";

/** anyba.se/mobile: same as /download, at the address people know from other apps. */
export const Route = createFileRoute("/mobile")({
  head: () => ({
    meta: [
      { title: `Download — ${PRODUCT_NAME}` },
      { name: "robots", content: "noindex" },
      { property: "og:url", content: `${SITE_URL}/mobile` },
    ],
  }),
  component: DownloadRedirect,
});
