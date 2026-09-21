import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/manrope";
import { App } from "./App";
import { initMonitoring } from "./monitoring";
import "./styles.css";
import "./cabinet-refresh.css";
import "./admin/AdminWorkspacePages.css";
import "./cabinet-motion.css";
import "./admin/TaskDialogMotion.css";
import "./theme-graphite.css";
import "./theme-palettes.css";
import "./admin/palette-routing-theme.css";
import "./sidebar-position.css";

initMonitoring("admin");

const root = document.getElementById("root");

if (!root) {
  throw new Error("Root element is missing.");
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
