import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";

export interface PageRouteNavigateOptions {
  /** Replace the current history entry, e.g. when correcting an invalid or stale URL. */
  replace?: boolean;
}

/**
 * The part of the URL that belongs to the current cabinet page: the path segments after the page's own path,
 * e.g. `["agents", "<id>", "instructions"]` for `/cabinet/p/<project>/ai/agents/<id>/instructions`.
 */
export interface PageRoute {
  segments: readonly string[];
  /** Resolves once the new segments are current, for work that must follow the URL change. */
  navigate: (segments: readonly string[], options?: PageRouteNavigateOptions) => void | Promise<void>;
}

export const PageRouteContext = createContext<PageRoute | null>(null);

export function sameSegments(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((segment, index) => segment === right[index]);
}

export function useMemoryPageRoute(initialSegments: readonly string[]): PageRoute {
  const [segments, setSegments] = useState(initialSegments);
  const navigate = useCallback((next: readonly string[]) => {
    setSegments((current) => sameSegments(current, next) ? current : [...next]);
  }, []);
  return useMemo(() => ({ segments, navigate }), [navigate, segments]);
}

/** A page rendered outside the cabinet router, e.g. in isolated tests, keeps its navigation in memory. */
export function usePageRoute(): PageRoute {
  const route = useContext(PageRouteContext);
  const memoryRoute = useMemoryPageRoute([]);
  return route ?? memoryRoute;
}

/**
 * Replaces the URL with the one for what the page actually shows, e.g. when it names a missing record or an
 * unknown tab. `ready` should wait until the records the URL can name have loaded.
 */
export function useCanonicalPageRoute(route: PageRoute, canonicalSegments: readonly string[], ready = true) {
  useEffect(() => {
    if (ready && !sameSegments(route.segments, canonicalSegments)) void route.navigate(canonicalSegments, { replace: true });
  });
}
