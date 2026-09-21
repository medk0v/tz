import { useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import type { WidgetLauncher } from "../api";
import { MessageBody } from "../MessageBody";
import { WidgetButtonIcon } from "../WidgetButtonIcon";
import { widgetFontStack } from "../widget-font";
import {
  widgetThemeCssVariables,
  type ResolvedWidgetTheme,
  type WidgetColorScheme,
} from "../widget-theme";
import widgetStyles from "../widget-styles.css?inline";
import messageBodyStyles from "../message-body.css?inline";
import "./WidgetAppearancePreview.css";

interface WidgetAppearancePreviewProps {
  active: boolean;
  position: WidgetLauncher["position"];
  theme: ResolvedWidgetTheme;
  colorScheme: WidgetColorScheme;
  supportName: string;
  onlineNow: string;
  greeting: string;
  showGreeting: boolean;
  messageText: string;
  replyText: string;
  ratingPrompt: string;
  messagePlaceholder: string;
  footerText: string;
  attachmentsEnabled: boolean;
  title: string;
}

const PREVIEW_DOCUMENT = '<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"></head><body><div id="app"></div></body></html>';

export function WidgetAppearancePreview({
  active,
  position,
  theme,
  colorScheme,
  supportName,
  onlineNow,
  greeting,
  showGreeting,
  messageText,
  replyText,
  ratingPrompt,
  messagePlaceholder,
  footerText,
  attachmentsEnabled,
  title,
}: WidgetAppearancePreviewProps) {
  const [frameRoot, setFrameRoot] = useState<HTMLElement | null>(null);
  const themeStyle = {
    fontFamily: widgetFontStack(theme.font_family),
    ...widgetThemeCssVariables(theme, colorScheme),
  } as CSSProperties;

  return (
    <iframe
      className="widget-appearance-preview"
      title={title}
      sandbox="allow-same-origin"
      srcDoc={PREVIEW_DOCUMENT}
      tabIndex={-1}
      onLoad={(event) => {
        setFrameRoot(event.currentTarget.contentDocument?.getElementById("app") ?? null);
      }}
    >
      {frameRoot && createPortal(
        <>
          {/* The iframe keeps the real widget styles and media queries isolated from the cabinet. */}
          <style>{widgetStyles}</style>
          <style>{messageBodyStyles}</style>
          <div className={`widget-stage widget-stage--open${position === "bottom_left" ? " widget-stage--bottom-left" : ""}`} data-theme={colorScheme} style={themeStyle} inert>
            <main key={colorScheme} className="widget-shell">
              <header className="widget-header">
                <div className="widget-brand-mark">
                  <svg aria-hidden="true" viewBox="0 0 28 28">
                    <path className="chat-glyph-outline" d="M6.2 5.5h15.6a3.7 3.7 0 0 1 3.7 3.7v7.3a3.7 3.7 0 0 1-3.7 3.7h-8.3L8 23.7v-3.5H6.2a3.7 3.7 0 0 1-3.7-3.7V9.2a3.7 3.7 0 0 1 3.7-3.7Z" />
                    <circle className="chat-glyph-dot" cx="9" cy="13" r="1" />
                    <circle className="chat-glyph-dot" cx="14" cy="13" r="1" />
                    <circle className="chat-glyph-dot" cx="19" cy="13" r="1" />
                  </svg>
                </div>
                <div className="widget-title">
                  <strong>{supportName}</strong>
                  <span><i />{onlineNow}</span>
                </div>
                <div className="widget-header-actions" aria-hidden="true">
                  <button type="button" className="widget-header-button widget-sound-button" tabIndex={-1}>
                    <svg viewBox="0 0 24 24">
                      <path d="M5 10v4h3l4 3V7l-4 3H5Z" />
                      <path d="M16 9.5a4 4 0 0 1 0 5m2-7.5a7 7 0 0 1 0 10" />
                    </svg>
                  </button>
                  <button type="button" className="widget-header-button widget-close-button" tabIndex={-1}>
                    <svg viewBox="0 0 24 24"><path d="m7 7 10 10M17 7 7 17" /></svg>
                  </button>
                </div>
              </header>
              <div className="widget-messages">
                {showGreeting && greeting && (
                  <article className="widget-message widget-message--outbound">
                    <div style={{ whiteSpace: "pre-wrap" }}>{greeting}</div>
                    <time>12:00</time>
                  </article>
                )}
                <article className="widget-message widget-message--inbound">
                  <div style={{ whiteSpace: "pre-wrap" }}>{messageText}</div>
                  <time>12:01</time>
                </article>
                {active && theme.reply_typing_effect && (
                  <article className="widget-message widget-message--outbound">
                    <MessageBody
                      key={`reply-typing-${theme.reply_typing_effect}`}
                      body={replyText}
                      animate={theme.reply_typing_effect}
                    />
                    <time>12:02</time>
                  </article>
                )}
                {ratingPrompt && (
                  <section className="widget-rating">
                    <form onSubmit={(event) => event.preventDefault()}>
                      <p className="widget-rating-prompt">{ratingPrompt}</p>
                      <div className="widget-stars" aria-hidden="true">
                        {[1, 2, 3, 4, 5].map((rating) => (
                          <button key={rating} type="button" className="widget-star widget-star--active" tabIndex={-1}>
                            <svg viewBox="0 0 24 24">
                              <path d="m12 2.8 2.75 5.57 6.15.9-4.45 4.33 1.05 6.12L12 16.83l-5.5 2.89 1.05-6.12L3.1 9.27l6.15-.9L12 2.8Z" />
                            </svg>
                          </button>
                        ))}
                      </div>
                    </form>
                  </section>
                )}
              </div>
              <div className={`widget-composer${attachmentsEnabled ? " widget-composer--attachments" : ""}`}>
                {attachmentsEnabled && (
                  <button type="button" className="widget-attachment-button" tabIndex={-1} aria-hidden="true">
                    <svg viewBox="0 0 24 24">
                      <path d="m20.5 11.5-8.1 8.1a6 6 0 0 1-8.5-8.5l9-9a4 4 0 0 1 5.7 5.7l-9.1 9.1a2 2 0 0 1-2.8-2.8l8.4-8.4" />
                    </svg>
                  </button>
                )}
                <textarea rows={1} placeholder={messagePlaceholder} readOnly tabIndex={-1} aria-label={messagePlaceholder} />
                <button type="button" className="widget-send-button" disabled tabIndex={-1} aria-hidden="true">
                  <WidgetButtonIcon icon={theme.send_icon} />
                </button>
              </div>
              {footerText.trim() && <footer className="widget-footer">{footerText}</footer>}
            </main>
          </div>
        </>,
        frameRoot,
      )}
    </iframe>
  );
}
