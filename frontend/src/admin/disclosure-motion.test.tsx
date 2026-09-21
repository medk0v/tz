import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AnimatedDetails } from "../AnimatedDetails";
import { AnimatedDisclosure } from "../AnimatedDisclosure";

const animations: Array<{ finish: () => void; cancel: ReturnType<typeof vi.fn>; frames: Keyframe[] }> = [];
let frames: Map<number, FrameRequestCallback>;
let frameId = 0;
let reduced = false;
const preferenceListeners = new Set<() => void>();

beforeEach(() => {
  animations.length = 0;
  frames = new Map();
  reduced = false;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { frames.set(++frameId, callback); return frameId; });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
  vi.stubGlobal("matchMedia", () => ({ matches: reduced, addEventListener: (_: string, fn: () => void) => preferenceListeners.add(fn), removeEventListener: (_: string, fn: () => void) => preferenceListeners.delete(fn) }));
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    return { height: this instanceof HTMLDetailsElement && !this.open ? 24 : 120 } as DOMRect;
  });
  vi.stubGlobal("Animation", class {});
  Object.defineProperty(HTMLElement.prototype, "animate", { configurable: true, value: vi.fn((keyframes: Keyframe[]) => {
    let resolve!: () => void;
    let reject!: (error: Error) => void;
    const finished = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
    const cancel = vi.fn(() => reject(new Error("cancelled")));
    animations.push({ finish: resolve, cancel, frames: keyframes });
    return { finished, cancel };
  }) });
});
afterEach(() => { cleanup(); delete (HTMLElement.prototype as Partial<HTMLElement>).animate; preferenceListeners.clear(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
function nextFrame() { act(() => { const callbacks = [...frames.values()]; frames.clear(); callbacks.forEach((fn) => fn(0)); }); }
async function finish() { await act(async () => { animations.at(-1)!.finish(); }); }

it("animates native details in both directions and restores automatic layout", async () => {
  const { container } = render(<AnimatedDetails><summary>Help</summary><p>Explanation</p></AnimatedDetails>);
  const details = container.querySelector("details")!;
  fireEvent.click(screen.getByText("Help")); nextFrame();
  expect(details.open).toBe(true);
  expect(animations[0].frames).toEqual([{ height: "24px" }, { height: "120px" }]);
  await finish();
  expect(details.style.overflow).toBe("");
  fireEvent.click(screen.getByText("Help")); nextFrame();
  expect(details.open).toBe(true);
  expect(animations[1].frames).toEqual([{ height: "120px" }, { height: "24px" }]);
  await finish();
  expect(details.open).toBe(false);
  expect(details.style.height).toBe("");
});

it("cancels an interrupted close so a quick reopen cannot be closed by the old animation", async () => {
  const { container } = render(<AnimatedDetails open><summary>Help</summary><p>Explanation</p></AnimatedDetails>);
  fireEvent.click(screen.getByText("Help")); nextFrame();
  const closing = animations[0];
  fireEvent.click(screen.getByText("Help")); nextFrame();
  expect(closing.cancel).toHaveBeenCalled();
  await act(async () => closing.finish());
  await finish();
  expect(container.querySelector("details")!.open).toBe(true);
});

it("hides exiting content from accessibility immediately and unmounts only after exit", async () => {
  const { rerender, container } = render(<AnimatedDisclosure open motion="fade"><button>Option</button></AnimatedDisclosure>);
  await finish();
  rerender(<AnimatedDisclosure open={false} motion="fade"><button>Option</button></AnimatedDisclosure>);
  expect(container.querySelector("button")).not.toBeNull();
  expect(screen.queryByRole("button", { name: "Option" })).not.toBeInTheDocument();
  expect(container.firstElementChild).toHaveAttribute("inert");
  await finish();
  expect(container.querySelector("button")).toBeNull();
});

it("keeps mounted form state and respects reduced motion, including a mid-animation change", async () => {
  const { rerender } = render(<AnimatedDisclosure open keepMounted><input aria-label="Draft" defaultValue="" /></AnimatedDisclosure>);
  fireEvent.change(screen.getByLabelText("Draft"), { target: { value: "Keep this" } });
  await finish();
  rerender(<AnimatedDisclosure open={false} keepMounted><input aria-label="Draft" defaultValue="" /></AnimatedDisclosure>);
  act(() => { reduced = true; for (const listener of preferenceListeners) listener(); });
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  const count = animations.length;
  rerender(<AnimatedDisclosure open keepMounted><input aria-label="Draft" defaultValue="" /></AnimatedDisclosure>);
  expect(screen.getByRole("textbox")).toHaveValue("Keep this");
  expect(animations).toHaveLength(count);
});

it("does not hijack links or nested summaries and follows controlled open changes", async () => {
  const content = <><summary>Outer <button type="button">Action</button></summary><AnimatedDetails><summary>Inner</summary><p>Nested</p></AnimatedDetails></>;
  const { container, rerender } = render(<AnimatedDetails open>{content}</AnimatedDetails>);
  fireEvent.click(screen.getByRole("button", { name: "Action" }));
  expect(animations).toHaveLength(0);
  fireEvent.click(screen.getByText("Inner")); nextFrame(); await finish();
  expect(container.querySelectorAll("details")[0].open).toBe(true);
  rerender(<AnimatedDetails open={false}>{content}</AnimatedDetails>); nextFrame(); await finish();
  expect(container.querySelectorAll("details")[0].open).toBe(false);
});
