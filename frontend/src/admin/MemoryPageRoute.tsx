import type { ReactNode } from "react";
import { PageRouteContext, useMemoryPageRoute, usePageRoute } from "./page-route";

/** Keeps a page's nested path in memory instead of the URL, e.g. for a page embedded in another page or in tests. */
export function MemoryPageRoute({ initialSegments = [], children }: {
  initialSegments?: readonly string[];
  children: ReactNode;
}) {
  const route = useMemoryPageRoute(initialSegments);
  return <PageRouteContext.Provider value={route}>{children}</PageRouteContext.Provider>;
}

/** Shows the nested path of the surrounding page route as `a/b/c` in `data-testid="page-route"`, for tests. */
export function CurrentPageRoute() {
  const { segments } = usePageRoute();
  return <span data-testid="page-route" hidden>{segments.join("/")}</span>;
}
