import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useContentMotion } from "./useContentMotion";

function Panel({ selection, visible = true }: { selection: string; visible?: boolean }) {
  const ref = useContentMotion<HTMLDivElement>(selection);
  return visible ? <div ref={ref}><input aria-label="Draft" defaultValue="" /></div> : null;
}

function motionEnvironment(reduced = false) {
  const preference = Object.assign(new EventTarget(), { matches: reduced });
  vi.stubGlobal("matchMedia", () => preference);
  const animations: { cancel: ReturnType<typeof vi.fn> }[] = [];
  const animate = vi.fn(() => {
    const animation = { cancel: vi.fn(), finished: new Promise<void>(() => {}) };
    animations.push(animation);
    return animation;
  });
  Object.defineProperty(HTMLElement.prototype, "animate", { configurable: true, value: animate });
  return { preference, animations, animate };
}

afterEach(() => {
  cleanup();
  Reflect.deleteProperty(HTMLElement.prototype, "animate");
  vi.unstubAllGlobals();
});

describe("content motion", () => {
  it("keeps the draft and focus, cancels interrupted motion, and ignores ordinary rerenders", () => {
    const { animate, animations } = motionEnvironment();
    const { rerender, unmount } = render(<Panel selection="first" />);
    const field = screen.getByLabelText("Draft");
    field.focus();
    fireEvent.change(field, { target: { value: "Unsaved instructions" } });
    rerender(<Panel selection="first" />);
    expect(animate).toHaveBeenCalledTimes(1);
    rerender(<Panel selection="second" />);
    expect(animations[0].cancel).toHaveBeenCalledOnce();
    expect(animate).toHaveBeenCalledTimes(2);
    expect(screen.getByLabelText("Draft")).toBe(field);
    expect(field).toHaveFocus();
    expect(field).toHaveValue("Unsaved instructions");
    unmount();
    expect(animations[1].cancel).toHaveBeenCalledOnce();
  });

  it("skips motion for reduced motion and stops an active animation when the preference changes", () => {
    const { preference, animate, animations } = motionEnvironment(true);
    const { rerender } = render(<Panel selection="first" />);
    expect(animate).not.toHaveBeenCalled();
    preference.matches = false;
    rerender(<Panel selection="second" />);
    expect(animate).toHaveBeenCalledOnce();
    act(() => { preference.matches = true; preference.dispatchEvent(new Event("change")); });
    expect(animations[0].cancel).toHaveBeenCalledOnce();
    rerender(<Panel selection="third" />);
    expect(animate).toHaveBeenCalledOnce();
  });

  it("keeps the form usable when the animation API is unavailable", () => {
    render(<Panel selection="first" />);
    fireEvent.change(screen.getByLabelText("Draft"), { target: { value: "Still editable" } });
    expect(screen.getByLabelText("Draft")).toHaveValue("Still editable");
  });

  it("animates a panel mounted after loading even when selection has not changed", () => {
    const { animate, animations } = motionEnvironment();
    const { rerender } = render(<Panel selection="preset" visible={false} />);
    expect(animate).not.toHaveBeenCalled();
    rerender(<Panel selection="preset" />);
    expect(animate).toHaveBeenCalledOnce();
    rerender(<Panel selection="preset" visible={false} />);
    expect(animations[0].cancel).toHaveBeenCalledOnce();
    rerender(<Panel selection="preset" />);
    expect(animate).toHaveBeenCalledTimes(2);
  });
});
