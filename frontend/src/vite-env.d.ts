/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_ENTRY_MODE?: "landing" | "login";
  readonly VITE_PRODUCT_EDITION?: "standard" | "lite";
  readonly VITE_SENTRY_DSN?: string;
  readonly VITE_SENTRY_ENVIRONMENT?: string;
  readonly VITE_SENTRY_RELEASE?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
