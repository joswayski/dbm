import { createFileRoute } from "@tanstack/react-router";
import { Privacy } from "../components/Privacy";
import { PRODUCT_NAME, SITE_URL } from "../site";

/** anyba.se/privacy, the privacy policy URL given to Google Play and the App Store. */
export const Route = createFileRoute("/privacy")({
  head: () => ({
    meta: [
      { title: `Privacy policy — ${PRODUCT_NAME}` },
      { name: "description", content: `${PRODUCT_NAME} does not collect, sell, or share your data.` },
      { property: "og:url", content: `${SITE_URL}/privacy` },
    ],
  }),
  component: Privacy,
});
