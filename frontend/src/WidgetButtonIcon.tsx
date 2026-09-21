import type { WidgetTheme } from "./api";

export const WIDGET_LAUNCHER_ICONS = ["chat", "message_circle", "headphones"] as const;
export const WIDGET_SEND_ICONS = ["send", "arrow_up", "arrow_right"] as const;

type WidgetButtonIconName = NonNullable<WidgetTheme["launcher_icon"] | WidgetTheme["send_icon"]>;

const iconPaths = {
  message_circle: "M21 11.5a8.4 8.4 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.4 8.4 0 0 1-3.8-.9L3 21l1.9-5.7a8.4 8.4 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.4 8.4 0 0 1 3.8-.9h.5a8.5 8.5 0 0 1 8 8v.5Z",
  headphones: "M3 14v-3a9 9 0 0 1 18 0v3M3 13h3v8H5a2 2 0 0 1-2-2v-6Zm18 0h-3v8h1a2 2 0 0 0 2-2v-6Z",
  send: "m4 4 17 8-17 8 3.4-8L4 4Zm3.4 8H21",
  arrow_up: "M12 20V4m-7 7 7-7 7 7",
  arrow_right: "M4 12h16m-7-7 7 7-7 7",
};

export function WidgetButtonIcon({ icon }: { icon: WidgetButtonIconName }) {
  return (
    <svg aria-hidden="true" data-widget-icon={icon} viewBox={icon === "chat" ? "0 0 28 28" : "0 0 24 24"}>
      {icon === "chat" ? (
        <>
          <path className="chat-glyph-outline" d="M6.2 5.5h15.6a3.7 3.7 0 0 1 3.7 3.7v7.3a3.7 3.7 0 0 1-3.7 3.7h-8.3L8 23.7v-3.5H6.2a3.7 3.7 0 0 1-3.7-3.7V9.2a3.7 3.7 0 0 1 3.7-3.7Z" />
          <circle className="chat-glyph-dot chat-glyph-dot--one" cx="9" cy="13" r="1" />
          <circle className="chat-glyph-dot chat-glyph-dot--two" cx="14" cy="13" r="1" />
          <circle className="chat-glyph-dot chat-glyph-dot--three" cx="19" cy="13" r="1" />
        </>
      ) : <path d={iconPaths[icon]} />}
    </svg>
  );
}
