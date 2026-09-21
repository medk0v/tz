import type { Locale } from "../i18n";

const messages = {
  tables: ["Tables", "Таблицы", "Tabele"], chooseTable: ["Insert a table", "Вставить таблицу", "Inserează un tabel"],
  createTable: ["Create table", "Создать таблицу", "Creează tabel"], selectTable: ["Select table", "Выбрать таблицу", "Selectează tabelul"],
  existing: ["Existing tables", "Существующие таблицы", "Tabele existente"], name: ["Name", "Название", "Nume"],
  description: ["Description", "Описание", "Descriere"], icon: ["Icon", "Значок", "Pictogramă"], settings: ["Table settings", "Настройки таблицы", "Setările tabelului"],
  deleteTable: ["Delete table", "Удалить таблицу", "Șterge tabelul"], deleteTableConfirm: ["Delete this table and every record? This cannot be undone.", "Удалить таблицу и все записи? Это действие нельзя отменить.", "Ștergi tabelul și toate înregistrările? Acțiunea este ireversibilă."],
  allRecords: ["All records", "Все записи", "Toate înregistrările"], record: ["Record", "Запись", "Înregistrare"],
  addRecord: ["Add record", "Добавить запись", "Adaugă înregistrare"], openRecord: ["Open record", "Открыть запись", "Deschide înregistrarea"],
  deleteRecord: ["Delete record", "Удалить запись", "Șterge înregistrarea"], deleteRecordConfirm: ["Delete this record and its relations?", "Удалить запись и её связи?", "Ștergi înregistrarea și relațiile sale?"],
  recordBody: ["Record content", "Содержимое записи", "Conținutul înregistrării"], untitled: ["Untitled record", "Без названия", "Înregistrare fără titlu"],
  tooLarge: ["Record content must be 200,000 characters or fewer.", "Содержимое записи должно быть не длиннее 200 000 символов.", "Conținutul înregistrării poate avea cel mult 200.000 de caractere."],
  fields: ["Fields", "Поля", "Câmpuri"], addField: ["Add field", "Добавить поле", "Adaugă câmp"], editField: ["Field settings", "Настройки поля", "Setările câmpului"],
  deleteField: ["Delete field", "Удалить поле", "Șterge câmpul"], deleteFieldConfirm: ["Delete this field and its values? Related inverse fields may also be removed.", "Удалить поле и его значения? Обратные поля связи также могут быть удалены.", "Ștergi câmpul și valorile sale? Câmpurile inverse asociate pot fi eliminate."],
  type: ["Field type", "Тип поля", "Tipul câmpului"], text: ["Text", "Текст", "Text"], long_text: ["Long text", "Длинный текст", "Text lung"], number: ["Number", "Число", "Număr"],
  date: ["Date", "Дата", "Dată"], boolean: ["Checkbox", "Флажок", "Bifă"], single_select: ["Single select", "Один вариант", "O singură opțiune"],
  multi_select: ["Multi select", "Несколько вариантов", "Mai multe opțiuni"], url: ["URL", "Ссылка", "URL"], email: ["Email", "Email", "Email"],
  relation: ["Relation", "Связь", "Relație"], rollup: ["Rollup", "Агрегация", "Agregare"], options: ["Options", "Варианты", "Opțiuni"],
  addOption: ["Add option", "Добавить вариант", "Adaugă opțiune"], optionLabel: ["Option label", "Название варианта", "Eticheta opțiunii"],
  targetDatabase: ["Related table", "Связанная таблица", "Tabel asociat"], cardinality: ["Relationship", "Тип связи", "Tip de relație"],
  one_to_one: ["One to one", "Один к одному", "Unu la unu"], one_to_many: ["One to many", "Один ко многим", "Unu la mulți"], many_to_many: ["Many to many", "Многие ко многим", "Mulți la mulți"],
  many_to_one: ["Many to one", "Многие к одному", "Mulți la unu"],
  inverseName: ["Inverse field name", "Название обратного поля", "Numele câmpului invers"], inverseDefault: ["Related records", "Связанные записи", "Înregistrări asociate"],
  relationField: ["Relation field", "Поле связи", "Câmpul relației"], targetField: ["Target field", "Целевое поле", "Câmp țintă"], operation: ["Operation", "Операция", "Operație"],
  count: ["Count", "Количество", "Numără"], sum: ["Sum", "Сумма", "Sumă"], min: ["Minimum", "Минимум", "Minim"], max: ["Maximum", "Максимум", "Maxim"], show_values: ["Show values", "Показать значения", "Afișează valorile"],
  structuralFixed: ["Relation and rollup structure is fixed after creation.", "Структура связи и агрегации задаётся при создании.", "Structura relației și agregării se stabilește la creare."],
  calculated: ["Calculated automatically", "Вычисляется автоматически", "Calculat automat"], selected: ["Selected: {count}", "Выбрано: {count}", "Selectate: {count}"],
  noRelations: ["No linked records", "Нет связанных записей", "Nu există înregistrări asociate"], chooseRelations: ["Choose related records", "Выбрать связанные записи", "Alege înregistrări asociate"],
  views: ["Views", "Представления", "Vizualizări"], addView: ["New view", "Новое представление", "Vizualizare nouă"], editView: ["View settings", "Настройки представления", "Setările vizualizării"],
  deleteView: ["Delete view", "Удалить представление", "Șterge vizualizarea"], deleteViewConfirm: ["Delete this saved view? Records will stay in the table.", "Удалить представление? Записи останутся в таблице.", "Ștergi vizualizarea salvată? Înregistrările rămân în tabel."],
  filters: ["Filters", "Фильтры", "Filtre"], addFilter: ["Add filter", "Добавить фильтр", "Adaugă filtru"], sorts: ["Sorting", "Сортировка", "Sortare"], addSort: ["Add sort", "Добавить сортировку", "Adaugă sortare"],
  equals: ["Equals", "Равно", "Egal cu"], not_equals: ["Does not equal", "Не равно", "Diferit de"], contains: ["Contains", "Содержит", "Conține"], greater_than: ["Greater than", "Больше", "Mai mare decât"], less_than: ["Less than", "Меньше", "Mai mic decât"], is_empty: ["Is empty", "Пусто", "Este gol"], is_not_empty: ["Is not empty", "Не пусто", "Nu este gol"],
  asc: ["Ascending", "По возрастанию", "Crescător"], desc: ["Descending", "По убыванию", "Descrescător"], filterValue: ["Filter value", "Значение фильтра", "Valoare de filtrare"],
  columns: ["Columns", "Столбцы", "Coloane"], visible: ["Visible", "Показывать", "Vizibil"], pinned: ["Pinned", "Закрепить", "Fixat"], width: ["Width", "Ширина", "Lățime"],
  moveUp: ["Move up", "Выше", "Mută în sus"], moveDown: ["Move down", "Ниже", "Mută în jos"], remove: ["Remove", "Удалить", "Elimină"],
  search: ["Search records", "Поиск записей", "Caută înregistrări"], loading: ["Loading…", "Загрузка…", "Se încarcă…"], empty: ["No records yet", "Записей пока нет", "Nu există înregistrări încă"],
  emptyTables: ["No tables yet", "Таблиц пока нет", "Nu există tabele încă"], emptyFields: ["Add a field to start filling the table.", "Добавьте поле, чтобы заполнить таблицу.", "Adaugă un câmp pentru a completa tabelul."],
  previous: ["Previous page", "Предыдущая страница", "Pagina anterioară"], next: ["Next page", "Следующая страница", "Pagina următoare"], pagination: ["{from}–{to} of {total}", "{from}–{to} из {total}", "{from}–{to} din {total}"],
  reorderHelp: ["Row order can be changed in All records with an empty search.", "Порядок строк меняется во «Всех записях» без поискового запроса.", "Ordinea se poate schimba în Toate înregistrările fără căutare."],
  save: ["Save", "Сохранить", "Salvează"], saved: ["Saved", "Сохранено", "Salvat"], saving: ["Saving…", "Сохранение…", "Se salvează…"], close: ["Close", "Закрыть", "Închide"], cancel: ["Cancel", "Отмена", "Anulează"],
  discardConfirm: ["Discard your unsaved table changes?", "Отменить несохранённые изменения таблицы?", "Renunți la modificările nesalvate ale tabelului?"],
  conflict: ["This table or record changed. Your draft is preserved. Close and reload to review the latest version.", "Таблица или запись изменилась. Черновик остался в редакторе. Закройте его и обновите данные, чтобы увидеть актуальную версию.", "Tabelul sau înregistrarea s-a modificat. Ciorna este păstrată. Închide și reîncarcă pentru a vedea ultima versiune."],
  saveError: ["Could not save. Your draft is still here.", "Не удалось сохранить. Черновик остался в редакторе.", "Salvarea a eșuat. Ciorna este păstrată."],
  loadError: ["Could not load the table.", "Не удалось загрузить таблицу.", "Tabelul nu a putut fi încărcat."], retry: ["Retry", "Повторить", "Reîncearcă"], refresh: ["Refresh table", "Обновить таблицу", "Reîncarcă tabelul"],
  readOnly: ["Read only", "Только чтение", "Doar citire"], editCell: ["Edit {field}", "Изменить: {field}", "Editează {field}"], value: ["Value", "Значение", "Valoare"],
  yes: ["Yes", "Да", "Da"], no: ["No", "Нет", "Nu"], none: ["None", "Нет", "Niciunul"], invalid: ["Check the values before saving.", "Проверьте значения перед сохранением.", "Verifică valorile înainte de salvare."],
  insert: ["Insert", "Вставить", "Inserează"], chooseView: ["View", "Представление", "Vizualizare"], showAll: ["All fields", "Все поля", "Toate câmpurile"],
  duplicateView: ["Duplicate view", "Дублировать представление", "Duplică vizualizarea"], copySuffix: [" (copy)", " (копия)", " (copie)"],
} as const;

export function noteDatabasesText(locale: Locale) {
  const index = locale === "ru" ? 1 : locale === "ro" ? 2 : 0;
  return (key: keyof typeof messages, values: Record<string, string | number> = {}): string =>
    messages[key][index].replace(/\{(\w+)\}/g, (match, name: string) => String(values[name] ?? match));
}

export function noteDatabaseError(locale: Locale, error: unknown) {
  return noteDatabasesText(locale)(typeof error === "object" && error !== null && "status" in error && error.status === 409 ? "conflict" : "saveError");
}
