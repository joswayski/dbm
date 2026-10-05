import { createFileRoute } from "@tanstack/react-router";
import { NotFound } from "../components/NotFound";

/** Prerendered to /404.html, which Cloudflare serves for unknown paths. */
export const Route = createFileRoute("/404")({
  head: () => ({ meta: [{ title: "Not found — Anybase" }] }),
  component: NotFound,
});
