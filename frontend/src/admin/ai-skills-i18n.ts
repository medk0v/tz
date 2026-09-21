import type { Locale } from "../i18n";

const en = {
  hintLabel: "What are skills?", hintTitle: "Skills add knowledge to agents", hintBody: "A skill is a set of knowledge and instructions that an agent follows in conversations and tasks. One skill can be assigned to several agents.", hintExample: "For example, give an HR lawyer agent a skill based on your country’s Labour Code — upload the code via “Create with AI” — and assign it tasks such as drafting employment contracts, amendments and orders.",
  assignmentHelp: "Skills apply to this agent in the current project. Changes are saved automatically.", assignmentEmpty: "No skills yet. Create or import one in the Skills tab.", assignmentSaveAgentFirst: "Save the agent first, then select its skills here.", assignmentSaved: "Agent skills saved.", assignmentSaveError: "Could not save the selection. The previous skills are still assigned; try again.",
  title: "Skills", description: "Model connections, agents, skills and their access within the project.",
  create: "Create skill", import: "Import skill", importFile: "Skill file", importHelp: "Import instructions from SKILL.md or a .txt file. Scripts and other files from a skill package are not imported.",
  list: "Project skills", search: "Find a skill", empty: "No skills yet. Create a skill or import ready-made instructions.", noMatches: "No matching skills.", select: "Select a skill, create one or import a SKILL.md file.",
  newTitle: "New skill", name: "Skill name", purpose: "Description", instructions: "Skill instructions", instructionsHelp: "Assigned agents use these instructions in conversations and tasks.",
  agents: "Assign to agents", agentsHelp: "Select up to 64 agents. You can save the skill and assign agents later.", noAgents: "Create agents in the Agents tab to assign this skill.", unassigned: "No agents assigned", assigned: "Assigned agents: {count}",
  unavailable: "Unavailable agent", removeUnavailable: "Remove unavailable agent", agentsLoading: "Loading agents…",
  save: "Save skill", saving: "Saving…", saved: "Skill saved.", imported: "Instructions imported. Review the skill and save it.", deleted: "Skill deleted and removed from its agents.",
  delete: "Delete skill", deleteConfirm: "Delete “{name}”? The skill will be removed from all assigned agents.", discard: "Discard unsaved skill changes?", unsaved: "Unsaved changes", cancel: "Cancel", refresh: "Refresh skills",
  loading: "Loading skills…", importing: "Importing…", loadError: "Could not load skills. Try again.", saveError: "Could not save the skill. Your changes are still in the editor.", deleteError: "Could not delete the skill. Try again.", retry: "Try again",
  importError: "Could not read the file. Try again.", invalidFile: "Choose a non-empty .md or .txt instruction file with up to 50,000 characters.", invalidFields: "Enter a name and instructions. The limits are 200 characters for the name, 2,000 for the description and 50,000 for instructions.",
  createWithAi: "Create with AI", creatorTitle: "Create a skill with AI", creatorHelp: "Describe what the skill should do. Add text, images or books as source material if needed, then review and save the generated instructions.",
  goal: "What should the skill do?", goalPlaceholder: "For example: review customer requests using the attached guide and propose a response.",
  sourceText: "Source text (optional)", sourceTextHelp: "Up to 1,000,000 characters across pasted text and extracted documents. Large books may take several minutes to process.",
  sourceFiles: "Source files", addSources: "Add files", sourceFormats: "TXT, MD, text-based PDF, DOCX, EPUB, JPEG, PNG, WebP. Up to 10 files: 20 MB per document, 10 MB per image, 30 MB total.",
  pdfHelp: "If a PDF is rejected, export a new copy without object compression or convert it to TXT, DOCX or EPUB. For scans, use text recognition (OCR) or upload page images.",
  sourcePrivacy: "Materials are sent to the selected model to create instructions. Original files are not saved with the skill.",
  removeSource: "Remove {name}", connection: "Model connection", chooseConnection: "Select a connection", noConnection: "Add an active OpenAI or OpenAI-compatible connection with a default model in the Model connections tab.",
  connectionHelp: "Uses the connection’s default model. Images require a model that supports image input.", connectionsLoading: "Loading model connections…",
  generate: "Generate instructions", generating: "Analyzing materials and preparing the skill…", stopGeneration: "Cancel generation", generated: "Instructions are ready. Review them, assign agents if needed, and save the skill.",
  generationError: "Could not generate the skill. Your text and files are still here; you can try again.", generationCancelled: "Generation cancelled. Your text and files are still here.", discardSources: "Discard the unsaved text and files for creating this skill?",
  invalidGoal: "Describe what the skill should do in 1 to 10,000 characters.", sourceTextTooLong: "Source text can contain up to 1,000,000 characters.",
  sourceFileInvalid: "Choose a non-empty TXT, MD, PDF, DOCX, EPUB, JPEG, PNG or WebP file: {name}.", sourceFileTooLarge: "File is too large: {name}. Documents can be up to 20 MB, images up to 10 MB.", sourceFilesTooMany: "You can attach up to 10 files.", sourceFilesTooLarge: "The combined file size must not exceed 30 MB.",
} as const;
type SkillTextKey = keyof typeof en;
const ru: Record<SkillTextKey, string> = {
  hintLabel: "Что такое скиллы?", hintTitle: "Скиллы добавляют агентам знания", hintBody: "Скилл — это набор знаний и инструкций, которым агент следует в диалогах и задачах. Один скилл можно привязать к нескольким агентам.", hintExample: "Например, агенту-юристу из HR добавьте скилл по Трудовому кодексу вашей страны — загрузите текст кодекса через «Создать с ИИ» — и поручайте ему составлять трудовые договоры, дополнительные соглашения и приказы.",
  assignmentHelp: "Скиллы действуют для этого агента в текущем проекте. Изменения сохраняются автоматически.", assignmentEmpty: "Скиллов пока нет. Создайте или подключите их во вкладке «Скиллы».", assignmentSaveAgentFirst: "Сначала сохраните агента, затем выберите его скиллы здесь.", assignmentSaved: "Скиллы агента сохранены.", assignmentSaveError: "Не удалось сохранить выбор. Прежние скиллы остались подключены; попробуйте ещё раз.",
  title: "Скиллы", description: "Подключения моделей, агенты, скиллы и их доступ в рамках проекта.",
  create: "Создать скилл", import: "Подключить готовый", importFile: "Файл скилла", importHelp: "Загрузите инструкции из SKILL.md или файла .txt. Скрипты и другие файлы из пакета скилла не импортируются.",
  list: "Скиллы проекта", search: "Найти скилл", empty: "Скиллов пока нет. Создайте скилл или подключите готовые инструкции.", noMatches: "Скиллы не найдены.", select: "Выберите скилл, создайте новый или загрузите файл SKILL.md.",
  newTitle: "Новый скилл", name: "Название скилла", purpose: "Описание", instructions: "Инструкции скилла", instructionsHelp: "Привязанные агенты используют эти инструкции в диалогах и задачах.",
  agents: "Привязать к агентам", agentsHelp: "Выберите до 64 агентов. Можно сохранить скилл и привязать агентов позже.", noAgents: "Создайте агентов во вкладке «Агенты», чтобы привязать к ним скилл.", unassigned: "Агенты не привязаны", assigned: "Привязано агентов: {count}",
  unavailable: "Недоступный агент", removeUnavailable: "Убрать недоступного агента", agentsLoading: "Загрузка агентов…",
  save: "Сохранить скилл", saving: "Сохранение…", saved: "Скилл сохранён.", imported: "Инструкции загружены. Проверьте скилл и сохраните его.", deleted: "Скилл удалён и отвязан от агентов.",
  delete: "Удалить скилл", deleteConfirm: "Удалить скилл «{name}»? Он будет отвязан от всех агентов.", discard: "Отменить несохранённые изменения скилла?", unsaved: "Есть несохранённые изменения", cancel: "Отмена", refresh: "Обновить скиллы",
  loading: "Загрузка скиллов…", importing: "Загрузка файла…", loadError: "Не удалось загрузить скиллы. Попробуйте ещё раз.", saveError: "Не удалось сохранить скилл. Ваши изменения остались в редакторе.", deleteError: "Не удалось удалить скилл. Попробуйте ещё раз.", retry: "Повторить",
  importError: "Не удалось прочитать файл. Попробуйте ещё раз.", invalidFile: "Выберите непустой файл инструкций .md или .txt длиной до 50 000 символов.", invalidFields: "Введите название и инструкции. Лимиты: 200 символов для названия, 2 000 для описания и 50 000 для инструкций.",
  createWithAi: "Создать с ИИ", creatorTitle: "Создание скилла с ИИ", creatorHelp: "Опишите, что должен делать скилл. При необходимости добавьте тексты, картинки или книги, затем проверьте и сохраните готовые инструкции.",
  goal: "Что должен делать скилл?", goalPlaceholder: "Например: разбирать обращения клиентов по приложенному руководству и предлагать ответ.",
  sourceText: "Исходный текст (необязательно)", sourceTextHelp: "До 1 000 000 символов суммарно во вставленном тексте и документах. Обработка больших книг может занять несколько минут.",
  sourceFiles: "Материалы для скилла", addSources: "Добавить файлы", sourceFormats: "TXT, MD, PDF с текстом, DOCX, EPUB, JPEG, PNG, WebP. До 10 файлов: документ до 20 МБ, картинка до 10 МБ, всего до 30 МБ.",
  pdfHelp: "Если PDF не принимается, сохраните новую копию без сжатия объектов или конвертируйте в TXT, DOCX или EPUB. Для сканов используйте распознавание текста (OCR) или загрузите изображения страниц.",
  sourcePrivacy: "Материалы отправляются выбранной модели для подготовки инструкций. Исходные файлы не сохраняются в скилле.",
  removeSource: "Убрать {name}", connection: "Подключение модели", chooseConnection: "Выберите подключение", noConnection: "Во вкладке «Подключения моделей» добавьте активное подключение OpenAI или совместимого API и укажите модель по умолчанию.",
  connectionHelp: "Используется модель по умолчанию из подключения. Для картинок нужна модель с поддержкой изображений.", connectionsLoading: "Загрузка подключений моделей…",
  generate: "Подготовить инструкции", generating: "Анализируем материалы и готовим скилл…", stopGeneration: "Отменить генерацию", generated: "Инструкции готовы. Проверьте их, при необходимости привяжите агентов и сохраните скилл.",
  generationError: "Не удалось создать скилл. Ваш текст и файлы сохранены в форме — можно попробовать ещё раз.", generationCancelled: "Генерация отменена. Текст и файлы остались в форме.", discardSources: "Удалить несохранённый текст и файлы для создания скилла?",
  invalidGoal: "Опишите задачу скилла: от 1 до 10 000 символов.", sourceTextTooLong: "Исходный текст может содержать до 1 000 000 символов.",
  sourceFileInvalid: "Выберите непустой файл TXT, MD, PDF, DOCX, EPUB, JPEG, PNG или WebP: {name}.", sourceFileTooLarge: "Слишком большой файл: {name}. Документы — до 20 МБ, картинки — до 10 МБ.", sourceFilesTooMany: "Можно прикрепить до 10 файлов.", sourceFilesTooLarge: "Общий размер файлов не должен превышать 30 МБ.",
};
const ro: Record<SkillTextKey, string> = {
  hintLabel: "Ce sunt abilitățile?", hintTitle: "Abilitățile adaugă cunoștințe agenților", hintBody: "O abilitate este un set de cunoștințe și instrucțiuni pe care agentul le urmează în conversații și sarcini. O abilitate poate fi asociată mai multor agenți.", hintExample: "De exemplu, adăugați unui agent jurist din HR o abilitate bazată pe Codul muncii al țării dvs. — încărcați textul codului prin „Creează cu IA” — și încredințați-i întocmirea contractelor de muncă, a acordurilor adiționale și a ordinelor.",
  assignmentHelp: "Abilitățile se aplică acestui agent în proiectul curent. Modificările se salvează automat.", assignmentEmpty: "Nu există abilități. Creați sau importați una în fila Abilități.", assignmentSaveAgentFirst: "Salvați mai întâi agentul, apoi selectați abilitățile aici.", assignmentSaved: "Abilitățile agentului au fost salvate.", assignmentSaveError: "Selecția nu a putut fi salvată. Abilitățile anterioare au rămas asociate; încercați din nou.",
  title: "Abilități", description: "Conexiunile la modele, agenții, abilitățile și accesul lor în cadrul proiectului.",
  create: "Creează abilitate", import: "Importă abilitate", importFile: "Fișierul abilității", importHelp: "Importați instrucțiuni din SKILL.md sau dintr-un fișier .txt. Scripturile și celelalte fișiere din pachet nu sunt importate.",
  list: "Abilitățile proiectului", search: "Caută o abilitate", empty: "Nu există abilități. Creați una sau importați instrucțiuni existente.", noMatches: "Nicio abilitate găsită.", select: "Selectați o abilitate, creați una sau importați un fișier SKILL.md.",
  newTitle: "Abilitate nouă", name: "Numele abilității", purpose: "Descriere", instructions: "Instrucțiunile abilității", instructionsHelp: "Agenții asociați folosesc aceste instrucțiuni în conversații și sarcini.",
  agents: "Asociază cu agenți", agentsHelp: "Selectați până la 64 de agenți. Puteți salva abilitatea și asocia agenții mai târziu.", noAgents: "Creați agenți în fila Agenți pentru a asocia această abilitate.", unassigned: "Niciun agent asociat", assigned: "Agenți asociați: {count}",
  unavailable: "Agent indisponibil", removeUnavailable: "Elimină agentul indisponibil", agentsLoading: "Se încarcă agenții…",
  save: "Salvează abilitatea", saving: "Se salvează…", saved: "Abilitatea a fost salvată.", imported: "Instrucțiunile au fost importate. Verificați și salvați abilitatea.", deleted: "Abilitatea a fost ștearsă și eliminată de la agenții asociați.",
  delete: "Șterge abilitatea", deleteConfirm: "Ștergeți abilitatea „{name}”? Va fi eliminată de la toți agenții asociați.", discard: "Renunțați la modificările nesalvate ale abilității?", unsaved: "Modificări nesalvate", cancel: "Anulează", refresh: "Actualizează abilitățile",
  loading: "Se încarcă abilitățile…", importing: "Se importă…", loadError: "Abilitățile nu au putut fi încărcate. Încercați din nou.", saveError: "Abilitatea nu a putut fi salvată. Modificările au rămas în editor.", deleteError: "Abilitatea nu a putut fi ștearsă. Încercați din nou.", retry: "Reîncearcă",
  importError: "Fișierul nu a putut fi citit. Încercați din nou.", invalidFile: "Selectați un fișier .md sau .txt cu instrucțiuni, nevid, de maximum 50.000 de caractere.", invalidFields: "Introduceți numele și instrucțiunile. Limite: 200 de caractere pentru nume, 2.000 pentru descriere și 50.000 pentru instrucțiuni.",
  createWithAi: "Creează cu IA", creatorTitle: "Creează o abilitate cu IA", creatorHelp: "Descrieți ce trebuie să facă abilitatea. Adăugați texte, imagini sau cărți dacă este nevoie, apoi verificați și salvați instrucțiunile generate.",
  goal: "Ce trebuie să facă abilitatea?", goalPlaceholder: "De exemplu: analizează cererile clienților folosind ghidul atașat și propune un răspuns.",
  sourceText: "Text sursă (opțional)", sourceTextHelp: "Până la 1.000.000 de caractere în total, în textul introdus și documente. Procesarea cărților mari poate dura câteva minute.",
  sourceFiles: "Materiale sursă", addSources: "Adaugă fișiere", sourceFormats: "TXT, MD, PDF cu text, DOCX, EPUB, JPEG, PNG, WebP. Până la 10 fișiere: 20 MB per document, 10 MB per imagine, 30 MB în total.",
  pdfHelp: "Dacă PDF-ul este respins, exportați o copie nouă fără comprimarea obiectelor sau convertiți-l în TXT, DOCX ori EPUB. Pentru scanări, folosiți recunoașterea textului (OCR) sau încărcați imaginile paginilor.",
  sourcePrivacy: "Materialele sunt trimise modelului selectat pentru a crea instrucțiuni. Fișierele originale nu sunt salvate în abilitate.",
  removeSource: "Elimină {name}", connection: "Conexiune la model", chooseConnection: "Selectați o conexiune", noConnection: "În fila Conexiuni la modele, adăugați o conexiune activă OpenAI sau API compatibil și specificați modelul implicit.",
  connectionHelp: "Folosește modelul implicit al conexiunii. Pentru imagini este necesar un model care acceptă imagini.", connectionsLoading: "Se încarcă conexiunile la modele…",
  generate: "Generează instrucțiuni", generating: "Se analizează materialele și se pregătește abilitatea…", stopGeneration: "Anulează generarea", generated: "Instrucțiunile sunt gata. Verificați-le, asociați agenți dacă este nevoie și salvați abilitatea.",
  generationError: "Abilitatea nu a putut fi generată. Textul și fișierele au rămas în formular; puteți încerca din nou.", generationCancelled: "Generarea a fost anulată. Textul și fișierele au rămas în formular.", discardSources: "Renunțați la textul și fișierele nesalvate pentru crearea abilității?",
  invalidGoal: "Descrieți sarcina abilității folosind între 1 și 10.000 de caractere.", sourceTextTooLong: "Textul sursă poate conține până la 1.000.000 de caractere.",
  sourceFileInvalid: "Selectați un fișier TXT, MD, PDF, DOCX, EPUB, JPEG, PNG sau WebP nevid: {name}.", sourceFileTooLarge: "Fișier prea mare: {name}. Documentele pot avea până la 20 MB, imaginile până la 10 MB.", sourceFilesTooMany: "Puteți atașa până la 10 fișiere.", sourceFilesTooLarge: "Dimensiunea totală a fișierelor nu poate depăși 30 MB.",
};

export function aiSkillsText(locale: Locale) {
  const messages = locale === "ru" ? ru : locale === "ro" ? ro : en;
  return (key: SkillTextKey, values?: Record<string, string | number>): string => {
    let result: string = messages[key];
    for (const [name, value] of Object.entries(values ?? {})) result = result.replaceAll(`{${name}}`, String(value));
    return result;
  };
}
