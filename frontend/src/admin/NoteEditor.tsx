import { useEffect, useMemo, useRef, useState } from "react";
import {
  BlockTypeSelect, BoldItalicUnderlineToggles, CodeToggle, CreateLink, InsertTable,
  InsertThematicBreak, ListsToggle, MDXEditor, Separator, StrikeThroughSupSubToggles, UndoRedo,
  headingsPlugin, linkDialogPlugin, linkPlugin, listsPlugin, markdownShortcutPlugin, quotePlugin,
  tablePlugin, thematicBreakPlugin, toolbarPlugin, type MDXEditorMethods, type Translation,
} from "@mdxeditor/editor";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useI18n } from "../i18n";
import { notesEditorTranslation, notesText } from "./notes-i18n";
import "@mdxeditor/editor/style.css";

interface Props {
  value: string;
  onChange: (value: string) => void;
  readOnly: boolean;
}

function safeLink(value: string): boolean {
  try { return ["https:", "http:", "mailto:"].includes(new URL(value).protocol); }
  catch { return false; }
}

function Toolbar() {
  return <>
    <UndoRedo /><Separator /><BlockTypeSelect /><Separator />
    <BoldItalicUnderlineToggles options={["Bold", "Italic"]} />
    <StrikeThroughSupSubToggles options={["Strikethrough"]} /><CodeToggle /><Separator />
    <ListsToggle options={["bullet", "number", "check"]} /><CreateLink /><InsertTable /><InsertThematicBreak />
  </>;
}

export default function NoteEditor({ value, onChange, readOnly }: Props) {
  const { locale } = useI18n();
  const t = notesText(locale);
  const [sourceMode, setSourceMode] = useState(false);
  const [parseError, setParseError] = useState(false);
  const editorRef = useRef<MDXEditorMethods>(null);
  const lastValue = useRef(value);
  const translation = useMemo<Translation>(() => (key, fallback, interpolations) => {
    const template = key === "contentArea.editableMarkdown" ? notesText(locale)("body") : notesEditorTranslation(locale, key, fallback);
    return Object.entries(interpolations ?? {}).reduce(
      (text, [name, replacement]) => text.replaceAll(`{{${name}}}`, String(replacement)), template,
    );
  }, [locale]);
  const plugins = useMemo(() => [
    headingsPlugin(), listsPlugin(), quotePlugin(), thematicBreakPlugin(),
    linkPlugin({ validateUrl: safeLink }), linkDialogPlugin(), tablePlugin(), markdownShortcutPlugin(),
    toolbarPlugin({ toolbarClassName: "notes-editor-toolbar", toolbarContents: Toolbar }),
  ], []);

  useEffect(() => {
    if (value === lastValue.current) return;
    lastValue.current = value;
    editorRef.current?.setMarkdown(value);
  }, [value]);

  return <div className="notes-editor" role="group" aria-label={t("body")} onClickCapture={(event) => {
    const link = (event.target as HTMLElement).closest("a");
    if (link && !safeLink(link.getAttribute("href") ?? "")) event.preventDefault();
  }}>
    <div className="notes-editor-modes" role="group" aria-label={t("editorMode")}>
      <button type="button" aria-pressed={!sourceMode} onClick={() => { setParseError(false); setSourceMode(false); }}>{t("visual")}</button>
      <button type="button" aria-pressed={sourceMode} onClick={() => setSourceMode(true)}>{t("source")}</button>
    </div>
    {parseError && <p className="notes-inline-error" role="alert">{t("parseError")}</p>}
    {sourceMode ? <textarea className="notes-source" aria-label={t("body")} value={value} readOnly={readOnly} spellCheck={false}
      onChange={(event) => { lastValue.current = event.target.value; onChange(event.target.value); }} />
      : readOnly ? <div className="notes-prose notes-readonly"><ReactMarkdown remarkPlugins={[remarkGfm]} skipHtml
        components={{ a: ({ children, href }) => <a href={href && safeLink(href) ? href : undefined} target="_blank" rel="noopener noreferrer">{children}</a>, img: () => null }}>
        {value}
      </ReactMarkdown></div> : <MDXEditor
        ref={editorRef} markdown={value} plugins={plugins} className="notes-rich-editor" contentEditableClassName="notes-prose"
        placeholder={<span className="notes-editor-placeholder">{t("placeholder")}</span>} translation={translation} suppressHtmlProcessing trim={false} spellCheck
        onError={() => { setParseError(true); setSourceMode(true); }}
        onChange={(markdown, initialNormalize) => {
          if (initialNormalize) return;
          lastValue.current = markdown;
          onChange(markdown);
        }}
      />}
  </div>;
}
