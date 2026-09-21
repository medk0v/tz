import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MessageBody } from "../MessageBody";

describe("MessageBody", () => {
  afterEach(cleanup);

  it.each([undefined, "plain"] as const)("keeps %s messages as literal text", (format) => {
    const body = "**An existing message**\n[link](https://example.com) <b>plain</b>";
    const { container } = render(<MessageBody body={body} format={format} />);

    expect(container.textContent).toBe(body);
    expect(container.querySelector("strong, a, b")).toBeNull();
  });

  it("renders formatted replies, lists, quotes and paragraph line breaks", () => {
    const { container } = render(
      <MessageBody
        format="markdown"
        body={"**Bold** *italic* ~~removed~~ `reference`\nNext line\n\n- First\n- Second\n\n1. Ordered\n\n> Quoted reply"}
      />,
    );

    expect(screen.getByText("Bold").tagName).toBe("STRONG");
    expect(screen.getByText("italic").tagName).toBe("EM");
    expect(screen.getByText("removed").tagName).toBe("DEL");
    expect(screen.getByText("reference").tagName).toBe("CODE");
    expect(container.querySelector("p")?.textContent).toContain("reference\nNext line");
    expect(container.querySelectorAll("ul > li")).toHaveLength(2);
    expect(container.querySelector("ol > li")?.textContent).toBe("Ordered");
    expect(container.querySelector("blockquote")?.textContent).toContain("Quoted reply");
  });

  it.each(["chat", "document"] as const)("allows only absolute web and email links with opener protection in %s", (variant) => {
    render(
      <MessageBody
        format="markdown"
        variant={variant}
        body={"[Secure](https://example.com/help) [Web](http://example.com) [Email](mailto:support@example.com) [Script](javascript:alert%281%29) [Encoded](&#x6a;avascript:alert%281%29) [Data](data:text/html,attack) [Relative](/settings) [Protocol-relative](//example.com)"}
      />,
    );

    expect(screen.getAllByRole("link")).toHaveLength(3);
    expect(screen.getByRole("link", { name: "Secure" })).toHaveAttribute("href", "https://example.com/help");
    expect(screen.getByRole("link", { name: "Email" })).toHaveAttribute("href", "mailto:support@example.com");
    for (const link of screen.getAllByRole("link")) {
      expect(link).toHaveAttribute("target", "_blank");
      expect(link).toHaveAttribute("rel", "noopener noreferrer");
    }
    for (const label of ["Script", "Encoded", "Data", "Relative", "Protocol-relative"]) {
      expect(screen.getByText(label).closest("a")).toBeNull();
    }
  });

  it.each(["chat", "document"] as const)("never loads remote images or renders raw HTML in %s", (variant) => {
    const { container } = render(
      <MessageBody
        format="markdown"
        variant={variant}
        body={'![Receipt](https://example.com/tracking.png)\n\n<script>alert("unsafe")</script>\n\n<img src="https://example.com/raw.png" onerror="alert(1)">\n\n<iframe src="https://example.com"></iframe>\n\nVisible reply'}
      />,
    );

    expect(screen.getByText("Receipt")).toBeInTheDocument();
    expect(screen.getByText("Visible reply")).toBeInTheDocument();
    expect(container.querySelector("img, script, iframe, [onerror]")).toBeNull();
  });

  it("preserves document headings and tables only when explicitly enabled", () => {
    const body = "# Launch plan\n\n## Preparation\n\nA separate paragraph.\n\n| Stage | Owner |\n| --- | --- |\n| Launch | Team |";
    const { rerender } = render(<MessageBody body={body} format="markdown" />);
    expect(screen.queryByRole("heading")).not.toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
    rerender(<MessageBody body={body} format="markdown" variant="document" />);
    expect(screen.getByRole("heading", { level: 1, name: "Launch plan" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 2, name: "Preparation" })).toBeInTheDocument();
    expect(screen.getByText("A separate paragraph.").tagName).toBe("P");
    expect(screen.getByRole("columnheader", { name: "Owner" })).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "Team" })).toBeInTheDocument();
  });

  it("reserves the complete plain text and preserves whitespace while revealing intact graphemes", () => {
    const body = "  Hi\t👩🏽‍💻 e\u0301\n\n👨‍👩‍👧‍👦 🇲🇩  ";
    const { container } = render(<MessageBody body={body} animate />);
    const characters = Array.from(container.querySelectorAll(".message-typed-character"));

    expect(container.querySelector(".message-typed-accessible")?.textContent).toBe(body);
    expect(container.querySelector('[aria-hidden="true"]')?.textContent).toBe(body);
    expect(characters.map((node) => node.textContent)).toEqual([
      " ", " ", "H", "i", "\t", "👩🏽‍💻", " ", "e\u0301", "\n", "\n", "👨‍👩‍👧‍👦", " ", "🇲🇩", " ", " ",
    ]);
    expect(container.querySelector(".message-body--plain")).toBeInTheDocument();
  });

  it("keeps formatted text and complete accessible link names during the reveal", () => {
    const { container } = render(
      <MessageBody animate format="markdown" body={"**Hello** *there*\n\n- Visit [our **help** center](https://example.com/help)\n- Use `reference`\n\n> Thank you"} />,
    );

    const link = screen.getByRole("link", { name: "our help center" });
    expect(link).toHaveAttribute("href", "https://example.com/help");
    expect(link).toHaveAttribute("rel", "noopener noreferrer");
    expect(link.querySelector("strong .message-typed-character")).toBeInTheDocument();
    expect(container.querySelectorAll("ul > li")).toHaveLength(2);
    expect(container.querySelector("em .message-typed-accessible")?.textContent).toBe("there");
    expect(container.querySelector("code .message-typed-accessible")?.textContent).toBe("reference");
    expect(container.querySelector("blockquote .message-typed-accessible")?.textContent).toBe("Thank you");
  });

  it("preserves link and HTML safety when animating markdown", () => {
    const { container } = render(
      <MessageBody animate format="markdown" body={'[Safe](https://example.com) [Unsafe](javascript:alert%281%29) [Relative](/settings) ![Receipt](https://example.com/tracking.png)\n\n<script>alert("unsafe")</script>'} />,
    );

    expect(screen.getAllByRole("link")).toHaveLength(1);
    expect(screen.getByRole("link", { name: "Safe" })).toHaveAttribute("target", "_blank");
    expect(screen.getByText("Unsafe", { selector: ".message-typed-accessible" }).closest("a")).toBeNull();
    expect(screen.getByText("Relative", { selector: ".message-typed-accessible" }).closest("a")).toBeNull();
    expect(container.querySelector("img, script, iframe, [onerror]")).toBeNull();
  });

  it.each(["plain", "markdown"] as const)("bounds %s reveal nodes and duration for a long answer", (format) => {
    const body = "Long answer with emoji 👩🏽‍💻. ".repeat(250);
    const { container } = render(<MessageBody animate body={body} format={format} />);
    const characters = Array.from(container.querySelectorAll<HTMLElement>(".message-typed-character"));

    expect(characters.length).toBeGreaterThan(0);
    expect(characters.length).toBeLessThanOrEqual(300);
    expect(characters.map((node) => node.textContent).join("")).toBe(body.trimEnd() + (format === "plain" ? " " : ""));
    expect(Math.max(...characters.map((node) => Number.parseFloat(node.style.animationDelay))) + 120).toBeLessThanOrEqual(1200);
  });

  it.each(["plain", "markdown"] as const)("keeps existing %s animation nodes on unrelated rerenders", (format) => {
    const body = "Hello **there** 👩🏽‍💻";
    const { container, rerender } = render(<MessageBody animate body={body} format={format} />);
    const first = container.querySelector(".message-typed-character");

    rerender(<MessageBody animate body={body} format={format} />);

    expect(container.querySelector(".message-typed-character")).toBe(first);
    rerender(<MessageBody body={body} format={format} />);
    expect(container.querySelector(".message-typed-character")).toBeNull();
  });

  it("renders unusually fragmented markdown without adding hundreds of animation wrappers", () => {
    const { container } = render(<MessageBody animate format="markdown" body={"**word** ".repeat(301)} />);

    expect(container.querySelectorAll("strong")).toHaveLength(301);
    expect(container.querySelector(".message-typed-character")).toBeNull();
  });

  it("shows complete messages if the browser cannot segment graphemes", async () => {
    const descriptor = Object.getOwnPropertyDescriptor(Intl, "Segmenter")!;
    Object.defineProperty(Intl, "Segmenter", { configurable: true, value: undefined });
    vi.resetModules();
    try {
      const { MessageBody: LegacyMessageBody } = await import("../MessageBody");
      const body = "Hello 👩🏽‍💻";
      const { container, rerender } = render(<LegacyMessageBody animate body={body} />);
      expect(container.textContent).toBe(body);
      expect(container.querySelector(".message-typed-character")).toBeNull();

      rerender(<LegacyMessageBody animate format="markdown" body="**Hello** 👩🏽‍💻" />);
      expect(container.textContent).toBe(body);
      expect(container.querySelector("strong")?.textContent).toBe("Hello");
      expect(container.querySelector(".message-typed-character")).toBeNull();
    } finally {
      Object.defineProperty(Intl, "Segmenter", descriptor);
      vi.resetModules();
    }
  });
});
