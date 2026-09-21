/** @jsxImportSource preact */

import { render } from "preact";
import { App } from "./WidgetApp";
import { initMonitoring } from "./monitoring";
import "./widget-styles.css";

initMonitoring("widget");

const root = document.getElementById("app");

if (!root) {
  throw new Error("Widget root element is missing.");
}

render(<App />, root);
