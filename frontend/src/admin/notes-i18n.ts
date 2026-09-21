import type { Locale } from "../i18n";

const en = {
  title: "Notes", pages: "Pages", allPages: "All pages", favorites: "Favorites", search: "Search page titles",
  newPage: "New page", newChild: "Add subpage", untitled: "Untitled", empty: "No pages yet",
  emptyHelp: "Keep project ideas, decisions and working documents here.", selectPage: "Select a page",
  selectHelp: "Open a page from the list or create a new one.", noResults: "No matching pages", noFavorites: "No favorite pages yet",
  pageTitle: "Page title", body: "Page content", placeholder: "Start writing, or use Markdown shortcuts…",
  parent: "Parent page", root: "Top level", icon: "Page icon", noIcon: "No icon", favorite: "Add to favorites", unfavorite: "Remove from favorites",
  save: "Save", saving: "Saving…", saved: "Saved", unsaved: "Unsaved changes", duplicate: "Duplicate", copyTitle: "{title} (copy)",
  discard: "Discard changes", discardConfirm: "Discard your unsaved changes?", delete: "Delete page",
  deleteConfirm: "Delete “{title}”? This cannot be undone.", hasChildren: "Move or delete the subpages before deleting this page.",
  deleted: "Page deleted.", loading: "Loading…", loadError: "Could not load pages.", detailError: "Could not load this page.",
  notFound: "This page was deleted or is no longer available.", saveError: "Could not save. Your changes are still here.",
  deleteError: "Could not delete this page.", conflict: "This page changed since you opened it. Your draft is preserved. Reload the latest version or save your draft as a separate page.",
  reloadLatest: "Reload latest version", saveCopy: "Save as a copy", retry: "Retry", refresh: "Refresh pages", readOnly: "Read only",
  updated: "Updated {date}", showPages: "Show pages", hidePages: "Hide pages", expand: "Expand {title}", collapse: "Collapse {title}",
  visual: "Visual", source: "Markdown", editorMode: "Editor mode", parseError: "This content cannot be displayed in the visual editor. You can continue in Markdown mode.",
  movePage: "Move {title}", moveHelp: "Drag the handle to reorder or nest pages. On the handle: Alt + ↑/↓ reorders, Alt + → nests, Alt + ← moves out.",
  moveFiltered: "Clear search and select All pages to rearrange the tree.", moveRoot: "Move to top level", moving: "Moving page…", moved: "Page moved.",
  moveBefore: "Before {title}", moveAfter: "After {title}", moveInside: "Inside {title}", moveError: "Could not move the page. The tree and your draft are unchanged.",
  moveConflict: "The page changed. Save your draft or reopen the page before moving it again. Your draft is still here.",
  titleRequired: "Enter a page title.", tooLarge: "Page content is too large.", properties: "Page properties", back: "Back to pages",
};
type NotesTextKey = keyof typeof en;
const ru: Record<NotesTextKey, string> = {
  title: "Заметки", pages: "Страницы", allPages: "Все страницы", favorites: "Избранное", search: "Поиск по названиям страниц",
  newPage: "Новая страница", newChild: "Добавить подстраницу", untitled: "Без названия", empty: "Страниц пока нет",
  emptyHelp: "Храните здесь идеи, решения и рабочие документы проекта.", selectPage: "Выберите страницу",
  selectHelp: "Откройте страницу из списка или создайте новую.", noResults: "Страницы не найдены", noFavorites: "В избранном пока нет страниц",
  pageTitle: "Название страницы", body: "Содержимое страницы", placeholder: "Начните писать или используйте разметку Markdown…",
  parent: "Родительская страница", root: "Верхний уровень", icon: "Значок страницы", noIcon: "Без значка", favorite: "В избранное", unfavorite: "Убрать из избранного",
  save: "Сохранить", saving: "Сохранение…", saved: "Сохранено", unsaved: "Есть изменения", duplicate: "Дублировать", copyTitle: "{title} (копия)",
  discard: "Отменить изменения", discardConfirm: "Отменить несохранённые изменения?", delete: "Удалить страницу",
  deleteConfirm: "Удалить «{title}»? Это действие нельзя отменить.", hasChildren: "Сначала переместите или удалите подстраницы этой страницы.",
  deleted: "Страница удалена.", loading: "Загрузка…", loadError: "Не удалось загрузить страницы.", detailError: "Не удалось загрузить эту страницу.",
  notFound: "Страница удалена или больше недоступна.", saveError: "Не удалось сохранить. Ваши изменения остались в редакторе.",
  deleteError: "Не удалось удалить страницу.", conflict: "Страница изменилась после открытия. Ваш черновик сохранён в редакторе. Загрузите актуальную версию или сохраните черновик отдельной страницей.",
  reloadLatest: "Загрузить актуальную версию", saveCopy: "Сохранить копией", retry: "Повторить", refresh: "Обновить страницы", readOnly: "Только чтение",
  updated: "Обновлено {date}", showPages: "Показать страницы", hidePages: "Скрыть страницы", expand: "Развернуть {title}", collapse: "Свернуть {title}",
  visual: "Визуально", source: "Markdown", editorMode: "Режим редактора", parseError: "Это содержимое не поддерживается визуальным редактором. Продолжите работу в режиме Markdown.",
  movePage: "Переместить {title}", moveHelp: "Перетащите за ручку, чтобы изменить порядок или вложенность. На ручке: Alt + ↑/↓ меняет порядок, Alt + → вкладывает, Alt + ← выносит наружу.",
  moveFiltered: "Очистите поиск и выберите «Все страницы», чтобы изменить порядок дерева.", moveRoot: "Переместить на верхний уровень", moving: "Перемещение страницы…", moved: "Страница перемещена.",
  moveBefore: "Перед «{title}»", moveAfter: "После «{title}»", moveInside: "Внутрь «{title}»", moveError: "Не удалось переместить страницу. Дерево и черновик остались без изменений.",
  moveConflict: "Страница изменилась. Сохраните изменения или откройте её заново перед перемещением. Черновик остаётся в редакторе.",
  titleRequired: "Введите название страницы.", tooLarge: "Содержимое страницы слишком большое.", properties: "Свойства страницы", back: "К страницам",
};
const ro: Record<NotesTextKey, string> = {
  title: "Notițe", pages: "Pagini", allPages: "Toate paginile", favorites: "Favorite", search: "Caută în titlurile paginilor",
  newPage: "Pagină nouă", newChild: "Adaugă subpagină", untitled: "Fără titlu", empty: "Nu există pagini încă",
  emptyHelp: "Păstrează aici ideile, deciziile și documentele de lucru ale proiectului.", selectPage: "Selectează o pagină",
  selectHelp: "Deschide o pagină din listă sau creează una nouă.", noResults: "Nu s-au găsit pagini", noFavorites: "Nu există pagini favorite încă",
  pageTitle: "Titlul paginii", body: "Conținutul paginii", placeholder: "Începe să scrii sau folosește sintaxa Markdown…",
  parent: "Pagina părinte", root: "Nivel superior", icon: "Pictograma paginii", noIcon: "Fără pictogramă", favorite: "Adaugă la favorite", unfavorite: "Elimină din favorite",
  save: "Salvează", saving: "Se salvează…", saved: "Salvat", unsaved: "Modificări nesalvate", duplicate: "Duplică", copyTitle: "{title} (copie)",
  discard: "Renunță la modificări", discardConfirm: "Renunți la modificările nesalvate?", delete: "Șterge pagina",
  deleteConfirm: "Ștergi „{title}”? Această acțiune este ireversibilă.", hasChildren: "Mută sau șterge subpaginile înainte de a șterge această pagină.",
  deleted: "Pagina a fost ștearsă.", loading: "Se încarcă…", loadError: "Paginile nu au putut fi încărcate.", detailError: "Această pagină nu a putut fi încărcată.",
  notFound: "Pagina a fost ștearsă sau nu mai este disponibilă.", saveError: "Salvarea a eșuat. Modificările sunt încă în editor.",
  deleteError: "Pagina nu a putut fi ștearsă.", conflict: "Pagina s-a modificat de când ai deschis-o. Ciorna este păstrată. Reîncarcă ultima versiune sau salvează ciorna ca pagină separată.",
  reloadLatest: "Reîncarcă ultima versiune", saveCopy: "Salvează o copie", retry: "Reîncearcă", refresh: "Reîmprospătează paginile", readOnly: "Doar citire",
  updated: "Actualizat {date}", showPages: "Arată paginile", hidePages: "Ascunde paginile", expand: "Extinde {title}", collapse: "Restrânge {title}",
  visual: "Vizual", source: "Markdown", editorMode: "Modul editorului", parseError: "Acest conținut nu poate fi afișat în editorul vizual. Poți continua în modul Markdown.",
  movePage: "Mută {title}", moveHelp: "Trage mânerul pentru a reordona sau a încadra pagini. Pe mâner: Alt + ↑/↓ reordonează, Alt + → încadrează, Alt + ← mută în afară.",
  moveFiltered: "Șterge căutarea și alege Toate paginile pentru a rearanja arborele.", moveRoot: "Mută la nivelul superior", moving: "Se mută pagina…", moved: "Pagina a fost mutată.",
  moveBefore: "Înainte de {title}", moveAfter: "După {title}", moveInside: "În {title}", moveError: "Pagina nu a putut fi mutată. Arborele și ciorna au rămas neschimbate.",
  moveConflict: "Pagina s-a modificat. Salvează ciorna sau redeschide pagina înainte de a o muta din nou. Ciorna este păstrată.",
  titleRequired: "Introdu titlul paginii.", tooLarge: "Conținutul paginii este prea mare.", properties: "Proprietățile paginii", back: "La pagini",
};

export function notesText(locale: Locale) {
  const messages = locale === "ru" ? ru : locale === "ro" ? ro : en;
  return (key: NotesTextKey, values: Record<string, string> = {}): string =>
    messages[key].replace(/\{(\w+)\}/g, (match, name: string) => values[name] ?? match);
}

const editorTranslations: Record<string, [string, string]> = {
  "toolbar.undo": ["Отменить {{shortcut}}", "Anulează {{shortcut}}"],
  "toolbar.redo": ["Повторить {{shortcut}}", "Refă {{shortcut}}"],
  "toolbar.blockTypeSelect.placeholder": ["Стиль текста", "Stilul textului"],
  "toolbar.blockTypeSelect.selectBlockTypeTooltip": ["Выбрать стиль текста", "Alege stilul textului"],
  "toolbar.blockTypes.paragraph": ["Обычный текст", "Paragraf"],
  "toolbar.blockTypes.heading": ["Заголовок {{level}}", "Titlu {{level}}"],
  "toolbar.blockTypes.quote": ["Цитата", "Citat"],
  "toolbar.bold": ["Жирный", "Aldin"], "toolbar.removeBold": ["Убрать жирное начертание", "Elimină aldin"],
  "toolbar.italic": ["Курсив", "Cursiv"], "toolbar.removeItalic": ["Убрать курсив", "Elimină cursiv"],
  "toolbar.strikethrough": ["Зачёркнутый", "Tăiat"], "toolbar.removeStrikethrough": ["Убрать зачёркивание", "Elimină tăierea"],
  "toolbar.inlineCode": ["Код", "Cod"], "toolbar.removeInlineCode": ["Убрать формат кода", "Elimină formatul de cod"],
  "toolbar.bulletedList": ["Маркированный список", "Listă cu marcatori"],
  "toolbar.numberedList": ["Нумерованный список", "Listă numerotată"],
  "toolbar.checkList": ["Список задач", "Listă de verificare"],
  "toolbar.link": ["Добавить ссылку", "Adaugă link"], "toolbar.table": ["Добавить таблицу", "Adaugă tabel"],
  "toolbar.thematicBreak": ["Добавить разделитель", "Adaugă separator"],
  "toolbar.toggleGroup": ["Форматирование текста", "Formatarea textului"],
  "createLink.url": ["Адрес", "Adresă"], "createLink.urlPlaceholder": ["Вставьте адрес ссылки", "Introdu adresa linkului"],
  "createLink.text": ["Текст ссылки", "Textul linkului"], "createLink.textTooltip": ["Текст для читателя", "Textul afișat cititorului"],
  "createLink.title": ["Подсказка", "Indiciu"], "createLink.titleTooltip": ["Появится при наведении", "Apare la trecerea cursorului"],
  "createLink.saveTooltip": ["Сохранить ссылку", "Salvează linkul"], "createLink.cancelTooltip": ["Отменить изменение", "Anulează modificarea"],
  "linkPreview.copyToClipboard": ["Копировать ссылку", "Copiază linkul"], "linkPreview.copied": ["Скопировано", "Copiat"],
  "linkPreview.edit": ["Изменить ссылку", "Editează linkul"], "linkPreview.remove": ["Удалить ссылку", "Elimină linkul"],
  "dialogControls.cancel": ["Отмена", "Anulează"], "dialogControls.save": ["Сохранить", "Salvează"], "dialog.close": ["Закрыть", "Închide"],
  "table.columnMenu": ["Меню столбца", "Meniul coloanei"], "table.rowMenu": ["Меню строки", "Meniul rândului"],
  "table.insertColumnLeft": ["Добавить столбец слева", "Inserează coloană la stânga"],
  "table.insertColumnRight": ["Добавить столбец справа", "Inserează coloană la dreapta"],
  "table.insertRowAbove": ["Добавить строку выше", "Inserează rând deasupra"],
  "table.insertRowBelow": ["Добавить строку ниже", "Inserează rând dedesubt"],
  "table.deleteColumn": ["Удалить столбец", "Șterge coloana"], "table.deleteRow": ["Удалить строку", "Șterge rândul"],
  "table.deleteTable": ["Удалить таблицу", "Șterge tabelul"], "table.textAlignment": ["Выравнивание текста", "Alinierea textului"],
  "table.alignLeft": ["По левому краю", "La stânga"], "table.alignCenter": ["По центру", "La centru"], "table.alignRight": ["По правому краю", "La dreapta"],
};

export function notesEditorTranslation(locale: Locale, key: string, fallback: string): string {
  return locale === "en" ? fallback : editorTranslations[key]?.[locale === "ru" ? 0 : 1] ?? fallback;
}
