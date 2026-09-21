import { resolve } from "node:path";
import preact from "@preact/preset-vite";
import react from "@vitejs/plugin-react";
import { sentryVitePlugin } from "@sentry/vite-plugin";
import { defineConfig, loadEnv } from "vite";

export default defineConfig(({ command, mode }) => {
  const env = loadEnv(mode, process.cwd(), "");
  if (mode === "loader") {
    return {
      // The build ships no brand assets of its own.
      publicDir: false,
      build: {
        outDir: "dist/loader",
        emptyOutDir: true,
        lib: {
          entry: resolve(import.meta.dirname, "src/widget-loader.ts"),
          name: "TzWidgetLoader",
          formats: ["iife"],
          fileName: () => "widget-loader.js",
        },
        minify: "esbuild",
      },
    };
  }

  const widget = mode === "widget";
  const outDir = widget ? "dist/widget" : "dist/admin";
  const uploadSourceMaps =
    command === "build" &&
    Boolean(
      env.SENTRY_AUTH_TOKEN &&
        env.SENTRY_ORG &&
        env.SENTRY_PROJECT &&
        env.VITE_SENTRY_RELEASE,
    );

  return {
    publicDir: false,
    base: command === "build" && widget ? "/widget/" : "/",
    cacheDir: widget ? "node_modules/.vite-widget" : "node_modules/.vite-admin",
    plugins: [
      widget ? preact() : react(),
      ...(uploadSourceMaps
        ? [
            sentryVitePlugin({
              authToken: env.SENTRY_AUTH_TOKEN,
              org: env.SENTRY_ORG,
              project: env.SENTRY_PROJECT,
              url: env.SENTRY_URL,
              telemetry: false,
              release: {
                name: env.VITE_SENTRY_RELEASE,
                setCommits: false,
              },
              sourcemaps: {
                assets: `${outDir}/**/*`,
                filesToDeleteAfterUpload: `${outDir}/**/*.map`,
              },
            }),
          ]
        : []),
    ],
    build: {
      outDir,
      sourcemap: uploadSourceMaps ? "hidden" : false,
      rollupOptions: {
        input: resolve(
          import.meta.dirname,
          widget ? "widget.html" : "index.html",
        ),
      },
    },
    test: {
      environment: "jsdom",
      setupFiles: "./src/test-setup.ts",
      // Apply the widget's Preact aliases to the renderer's JSX runtime in tests.
      server: widget ? { deps: { inline: ["react-markdown"] } } : undefined,
      include: widget
        ? [
            "src/WidgetApp.test.tsx",
            "src/attachment-files.test.ts",
            "src/widget-bridge.test.ts",
            "src/widget-loader.test.ts",
          ]
        : [
            "src/App.test.tsx",
            "src/department-navigation.test.ts",
            "src/entry-mode.test.ts",
            "src/PublicRatingPage.test.tsx",
            "src/PublicNotesPage.test.tsx",
            "src/note-shares-api.test.ts",
            "src/note-databases-api.test.ts",
            "src/monitoring.test.ts",
            "src/admin/**/*.test.tsx",
            "src/attachment-files.test.ts",
          ],
    },
  };
});
