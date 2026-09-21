import "@testing-library/jest-dom/vitest";

if (typeof HTMLElement.prototype.scrollTo !== "function") {
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    value(optionsOrX: ScrollToOptions | number, y?: number) {
      if (typeof optionsOrX === "number") {
        this.scrollLeft = optionsOrX;
        this.scrollTop = y ?? 0;
        return;
      }
      if (optionsOrX.left !== undefined) this.scrollLeft = optionsOrX.left;
      if (optionsOrX.top !== undefined) this.scrollTop = optionsOrX.top;
    },
  });
}
