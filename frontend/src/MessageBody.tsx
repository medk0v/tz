import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Element, Root, Text } from "hast";
import "./message-body.css";

interface MessageBodyProps {
  body: string;
  format?: "plain" | "markdown";
  variant?: "chat" | "document";
  animate?: boolean;
}

const allowedElements = [
  "p", "strong", "em", "del", "code", "pre", "ol", "ul", "li", "blockquote", "a", "br", "img", "span",
];
const documentElements = [...allowedElements, "h1", "h2", "h3", "h4", "h5", "h6", "hr", "table", "thead", "tbody", "tr", "th", "td"];
const MAX_TYPED_CHUNKS = 300;
const TYPED_FADE_MS = 120;
const MAX_TYPING_MS = 1200;
const graphemeSegmenter = typeof Intl.Segmenter === "function"
  ? new Intl.Segmenter(undefined, { granularity: "grapheme" })
  : null;

function graphemes(text: string): string[] {
  return graphemeSegmenter
    ? Array.from(graphemeSegmenter.segment(text), ({ segment }) => segment)
    : [];
}

function accessibleText(parent: Root | Element): string {
  return parent.children.map((node) => {
    if (node.type === "text") return node.value;
    if (node.type !== "element") return "";
    if (node.tagName === "br") return "\n";
    if (node.tagName === "img") return String(node.properties.alt ?? "");
    return accessibleText(node);
  }).join("");
}

function typingInterval(total: number): number {
  return Math.min(32, (MAX_TYPING_MS - TYPED_FADE_MS) / Math.max(1, total - 1));
}

function typedChunks(segments: string[], size: number, interval: number, offset = 0) {
  const chunks: { value: string; delay: number }[] = [];
  for (let index = 0; index < segments.length; index += size) {
    const end = Math.min(index + size, segments.length);
    chunks.push({
      value: segments.slice(index, end).join(""),
      delay: (offset + end - 1) * interval,
    });
  }
  return chunks;
}

/** Transform the parsed tree before rendering; the full layout and accessible text stay intact. */
function rehypeTypedText() {
  return (tree: Root) => {
    if (!graphemeSegmenter) return;
    const textNodes: { parent: Root | Element; index: number; node: Text; segments: string[] }[] = [];
    const collect = (parent: Root | Element) => {
      if (parent.type === "element" && parent.tagName === "a") {
        // An explicit complete name preserves spaces across formatted label fragments.
        parent.properties.ariaLabel = accessibleText(parent);
      }
      parent.children.forEach((node, index) => {
        if (node.type === "text" && node.value.trim()) {
          textNodes.push({ parent, index, node, segments: graphemes(node.value) });
        } else if (node.type === "element") collect(node);
      });
    };
    collect(tree);
    // Extremely fragmented Markdown should not multiply its already large tree.
    if (textNodes.length > MAX_TYPED_CHUNKS) return;
    const total = textNodes.reduce((count, { segments }) => count + segments.length, 0);
    const interval = typingInterval(total);
    const size = Math.max(1, Math.ceil(total / (MAX_TYPED_CHUNKS - textNodes.length + 1)));
    let offset = 0;
    for (const { parent, index, node, segments } of textNodes) {
      const chunks = typedChunks(segments, size, interval, offset);
      parent.children[index] = {
        type: "element",
        tagName: "span",
        properties: { className: ["message-typed-text"] },
        children: [
          { type: "element", tagName: "span", properties: { className: ["message-typed-accessible"] }, children: [node] },
          {
            type: "element", tagName: "span", properties: { ariaHidden: "true" },
            children: chunks.map(({ value, delay }) => ({
              type: "element", tagName: "span",
              properties: { className: ["message-typed-character"], style: `animation-delay: ${delay}ms` },
              children: [{ type: "text", value }],
            })),
          },
        ],
      };
      offset += segments.length;
    }
  };
}

const typingPlugins = [rehypeTypedText];

function TypedPlainText({ body }: { body: string }) {
  const segments = graphemes(body);
  const chunks = typedChunks(
    segments,
    Math.max(1, Math.ceil(segments.length / MAX_TYPED_CHUNKS)),
    typingInterval(segments.length),
  );
  return (
    <span className="message-typed-text">
      <span className="message-typed-accessible">{body}</span>
      <span aria-hidden="true">
        {chunks.map(({ value, delay }, index) => (
          <span className="message-typed-character" style={{ animationDelay: `${delay}ms` }} key={index}>{value}</span>
        ))}
      </span>
    </span>
  );
}

function safeMessageUrl(value: string): string | undefined {
  try {
    const url = new URL(value);
    return ["http:", "https:", "mailto:"].includes(url.protocol) ? url.href : undefined;
  } catch {
    return undefined;
  }
}

const components: Components = {
  a: ({ href, children, "aria-label": label }) => href
    ? <a href={href} aria-label={label} target="_blank" rel="noopener noreferrer">{children}</a>
    : <span>{children}</span>,
  img: ({ alt }) => alt ? <span>{alt}</span> : null,
  table: ({ children }) => <div className="message-body-table"><table>{children}</table></div>,
};

export function MessageBody({ body, format = "plain", variant = "chat", animate = false }: MessageBodyProps) {
  if (format !== "markdown") {
    return <div className="message-body message-body--plain">{animate && graphemeSegmenter ? <TypedPlainText body={body} /> : body}</div>;
  }

  return (
    <div className={`message-body message-body--markdown${variant === "document" ? " message-body--document" : ""}`}>
      <Markdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={animate ? typingPlugins : undefined}
        allowedElements={variant === "document" ? documentElements : allowedElements}
        unwrapDisallowed
        skipHtml
        urlTransform={safeMessageUrl}
        components={components}
      >
        {body}
      </Markdown>
    </div>
  );
}
