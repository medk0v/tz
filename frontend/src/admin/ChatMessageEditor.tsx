import {
  BoldItalicUnderlineToggles,
  CodeToggle,
  CreateLink,
  ListsToggle,
  MDXEditor,
  Select,
  Separator,
  StrikeThroughSupSubToggles,
  UndoRedo,
  convertSelectionToNode$,
  currentBlockType$,
  linkDialogPlugin,
  linkPlugin,
  listsPlugin,
  markdownShortcutPlugin,
  maxLengthPlugin,
  quotePlugin,
  rootEditor$,
  toolbarPlugin,
  useTranslation,
  type MDXEditorMethods,
  type Translation,
} from "@mdxeditor/editor";
import { useCellValue, usePublisher } from "@mdxeditor/gurx";
import { $createQuoteNode } from "@lexical/rich-text";
import { $createParagraphNode, $getRoot, $getSelection, $isRangeSelection } from "lexical";
import { createContext, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { OperatorAuth } from "../api";
import { useI18n } from "../i18n";
import { AiReplySuggestionPicker, type AiReplySuggestionPickerProps } from "./AiReplySuggestionPicker";
import type { AiDraftSource } from "./ai-draft-sources";
import { ReplyTemplatePicker, type ReplyTemplatePickerProps } from "./ReplyTemplatePicker";
import "@mdxeditor/editor/style.css";
import "./ChatMessageEditor.css";

export interface ChatMessageEditorProps {
  value: string;
  onChange: (value: string) => void;
  label: string;
  placeholder: string;
  disabled?: boolean;
  templateSource?: { auth: OperatorAuth; inboxId: string };
  aiDraftSource?: AiDraftSource;
}

interface TemplateControls extends Omit<ReplyTemplatePickerProps, "onInsert"> {
  onInsert: (body: string, prepareInsertion: () => void) => boolean;
}

const ReplyTemplateContext = createContext<TemplateControls | null>(null);

interface AiSuggestionControls extends Omit<AiReplySuggestionPickerProps, "onInsert"> {
  onInsert: (body: string, prepareInsertion: () => void) => boolean;
}

const AiSuggestionContext = createContext<AiSuggestionControls | null>(null);

const russianTranslations: Record<string, string> = {
  "toolbar.undo": "Отменить {{shortcut}}",
  "toolbar.redo": "Повторить {{shortcut}}",
  "toolbar.blockTypeSelect.placeholder": "Стиль текста",
  "toolbar.blockTypeSelect.selectBlockTypeTooltip": "Выбрать стиль текста",
  "toolbar.blockTypes.paragraph": "Обычный текст",
  "toolbar.blockTypes.quote": "Цитата",
  "toolbar.bold": "Жирный",
  "toolbar.removeBold": "Убрать жирное начертание",
  "toolbar.italic": "Курсив",
  "toolbar.removeItalic": "Убрать курсив",
  "toolbar.strikethrough": "Зачёркнутый",
  "toolbar.removeStrikethrough": "Убрать зачёркивание",
  "toolbar.inlineCode": "Код",
  "toolbar.removeInlineCode": "Убрать формат кода",
  "toolbar.bulletedList": "Маркированный список",
  "toolbar.numberedList": "Нумерованный список",
  "toolbar.link": "Добавить ссылку",
  "toolbar.toggleGroup": "Форматирование текста",
  "createLink.url": "Адрес",
  "createLink.urlPlaceholder": "Вставьте адрес ссылки",
  "createLink.text": "Текст ссылки",
  "createLink.textTooltip": "Текст, который увидит клиент",
  "createLink.title": "Подсказка",
  "createLink.titleTooltip": "Появится при наведении",
  "createLink.saveTooltip": "Сохранить ссылку",
  "createLink.cancelTooltip": "Отменить изменение",
  "linkPreview.copyToClipboard": "Копировать ссылку",
  "linkPreview.copied": "Скопировано",
  "linkPreview.edit": "Изменить ссылку",
  "linkPreview.remove": "Удалить ссылку",
  "dialogControls.cancel": "Отмена",
  "dialogControls.save": "Сохранить",
  "dialog.close": "Закрыть",
};

function isSafeLink(value: string): boolean {
  try {
    return ["http:", "https:", "mailto:"].includes(new URL(value).protocol);
  } catch {
    return false;
  }
}

function ChatBlockTypeSelect() {
  const blockType = useCellValue(currentBlockType$);
  const convertSelection = usePublisher(convertSelectionToNode$);
  const t = useTranslation();

  return (
    <Select
      value={blockType === "paragraph" || blockType === "quote" ? blockType : ""}
      onChange={(value) => {
        if (value === "paragraph") convertSelection($createParagraphNode);
        if (value === "quote") convertSelection($createQuoteNode);
      }}
      triggerTitle={t("toolbar.blockTypeSelect.selectBlockTypeTooltip", "Select block type")}
      placeholder={t("toolbar.blockTypeSelect.placeholder", "Block type")}
      items={[
        { label: t("toolbar.blockTypes.paragraph", "Paragraph"), value: "paragraph" },
        { label: t("toolbar.blockTypes.quote", "Quote"), value: "quote" },
      ]}
    />
  );
}

function Toolbar() {
  const templates = useContext(ReplyTemplateContext);
  const aiSuggestion = useContext(AiSuggestionContext);
  const lexicalEditor = useCellValue(rootEditor$);
  function prepareTemplateInsertion() {
    lexicalEditor?.update(() => {
      const selection = $getSelection();
      const root = $getRoot();
      const last = root.getLastDescendant();
      if ($isRangeSelection(selection) && selection.isCollapsed() && last
        && last.getTextContentSize() > 0 && selection.anchor.key === last.getKey()
        && selection.anchor.offset === last.getTextContentSize()) {
        const paragraph = $createParagraphNode();
        root.append(paragraph);
        paragraph.selectStart();
      }
    });
  }
  return (
    <>
      <BoldItalicUnderlineToggles options={["Bold", "Italic"]} />
      <StrikeThroughSupSubToggles options={["Strikethrough"]} />
      <CodeToggle />
      <Separator />
      <ListsToggle options={["bullet", "number"]} />
      <ChatBlockTypeSelect />
      <CreateLink />
      <Separator />
      <UndoRedo />
      {templates && <ReplyTemplatePicker key={`${templates.inboxId}:${templates.disabled}`} {...templates}
        onInsert={(body) => templates.onInsert(body, prepareTemplateInsertion)} />}
      {aiSuggestion && <AiReplySuggestionPicker key={`${aiSuggestion.source.target}:${aiSuggestion.disabled}`} {...aiSuggestion}
        onInsert={(body) => aiSuggestion.onInsert(body, prepareTemplateInsertion)} />}
    </>
  );
}

export default function ChatMessageEditor({
  value,
  onChange,
  label,
  placeholder,
  disabled = false,
  templateSource,
  aiDraftSource,
}: ChatMessageEditorProps) {
  const { locale } = useI18n();
  const containerRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<MDXEditorMethods>(null);
  const insertionEnabled = useRef(!disabled);
  const [initialValue] = useState(value);
  const lastValue = useRef(value);
  const translation = useMemo<Translation>(() => (key, fallback, interpolations) => {
    if (key === "contentArea.editableMarkdown") return label;
    const template = locale === "ru" ? russianTranslations[key] ?? fallback : fallback;
    return Object.entries(interpolations ?? {}).reduce(
      (text, [name, replacement]) => text.replaceAll(`{{${name}}}`, String(replacement)),
      template,
    );
  }, [label, locale]);
  const plugins = useMemo(() => [
    listsPlugin(),
    quotePlugin(),
    linkPlugin({ validateUrl: isSafeLink }),
    linkDialogPlugin(),
    markdownShortcutPlugin(),
    maxLengthPlugin(10_000),
    toolbarPlugin({ toolbarClassName: "chat-rich-editor-toolbar", toolbarContents: Toolbar }),
  ], []);

  useEffect(() => {
    if (value === lastValue.current) return;
    lastValue.current = value;
    editorRef.current?.setMarkdown(value);
  }, [value]);

  useLayoutEffect(() => {
    insertionEnabled.current = !disabled;
    return () => { insertionEnabled.current = false; };
  }, [disabled]);

  function insertMarkdown(body: string, prepareInsertion: () => void): boolean {
    const editor = editorRef.current;
    if (!editor || disabled) return false;
    const current = editor.getMarkdown();
    if (current.length + body.length + (current.trim() ? 2 : 0) > 10_000) return false;
    requestAnimationFrame(() => {
      if (!insertionEnabled.current || editorRef.current !== editor) return;
      editor.focus(() => {
        if (insertionEnabled.current && editorRef.current === editor) {
          prepareInsertion();
          editor.insertMarkdown(body);
        }
      }, { defaultSelection: "rootEnd" });
    });
    return true;
  }

  return (
    <ReplyTemplateContext.Provider value={templateSource ? { ...templateSource, disabled, onInsert: insertMarkdown } : null}>
    <AiSuggestionContext.Provider value={aiDraftSource ? {
      source: aiDraftSource,
      draftBody: value,
      disabled,
      onInsert: insertMarkdown,
    } : null}>
    <div
      ref={containerRef}
      className="chat-message-editor"
      onKeyDownCapture={(event) => {
        if (event.key === "Enter" && (event.metaKey || event.ctrlKey)
          && !event.nativeEvent.isComposing && !disabled
          && (event.target as HTMLElement).closest('[contenteditable="true"]')) {
          event.preventDefault();
          event.stopPropagation();
          event.currentTarget.closest("form")?.requestSubmit();
        }
      }}
    >
      <MDXEditor
        ref={editorRef}
        markdown={initialValue}
        plugins={plugins}
        className="chat-rich-editor"
        contentEditableClassName="chat-rich-editor-content"
        placeholder={placeholder}
        translation={translation}
        readOnly={disabled}
        spellCheck
        suppressHtmlProcessing
        trim={false}
        onChange={(markdown, initialNormalize) => {
          if (initialNormalize) return;
          const text = containerRef.current?.querySelector("[contenteditable]")?.textContent;
          const nextValue = text?.trim() === "" ? "" : markdown;
          lastValue.current = nextValue;
          onChange(nextValue);
        }}
      />
    </div>
    </AiSuggestionContext.Provider>
    </ReplyTemplateContext.Provider>
  );
}
