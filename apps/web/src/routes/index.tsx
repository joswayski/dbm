import { createFileRoute } from "@tanstack/react-router";
import { Home } from "../components/Home";

export const Route = createFileRoute("/")({
  component: HomeRoute,
});

function HomeRoute() {
  return <Home latestChanges={__LATEST_CHANGES__} builtAt={__BUILT_AT__} />;
}
