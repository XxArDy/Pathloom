# Люди без коду й оркестратори — контракти (m10)

Запит власника 2026-11-20 (напрямки А і Б), план погоджено того ж дня. Протокол лишається `pathloom/1`; нове — необов'язкові поля, нові tools/команди, нові `capabilities`. Порядок: Б1 → Б2 → Б3 → А3 → А4 → А1 → решта.

Вже зроблено в m9 (лише перевірити й задокументувати як частину Б): Б4 (`report_usage {task_id, model, …}`, сума на етапі за моделлю), Б6 (`apply {propose: true}` + погодження на дошці), частина Б5 (`evidence.commit`).

## Б. Оркестратори (лише публічний протокол)

**Статус (2026-11-20):** Б1, Б2, Б3, Б5, Б8 зроблено в ядрі, CLI і MCP (UI — див. «Б1/Б2/Б5 UI» нижче). Відхилення від тексту нижче:
- Б1: `update_session` вимагає `task_id` або `stage` (інакше `INVALID_SPEC`); стан іншого виклику з `session` оновлює сесію в кожному етапі, який зачіпає запис журналу. `state_at` завжди ставить Pathloom.
- Б2: `allow_client_answers` не має CLI-перемикача, лише рядок у `project.toml` (`Project::set_allow_client_answers` для застосунку). `ALREADY_DECIDED` повертають і CLI/MCP/Telegram; `details`: `{by, at, state}` для погодження і `{by, at, choice}` для рішення. Помилка ядра `Error::AlreadyDecided`; застосунок поки показує її текстом помилки. Погодження без запису `approval` власник і далі схвалює напряму.
- Б7 (зроблено, 2026-11-20): `pathloom_core::link::{make_link(root, id?), parse_link(&str) -> LinkTarget{project, stage, task}}` (ручне %-кодування, ключі перевіряє `is_valid_key`, чужі схеми/хости відхиляються). CLI: `pathloom deeplink [ID]` (`--json` -> `{link}`; назва не `link`, бо `pathloom link FROM TO` уже є), MCP: `make_link {id?}` (read-only), capability `links` (обидва списки). Застосунок: `tauri-plugin-deep-link` (схема `pathloom` у `tauri.conf.json`, на Windows реєструє NSIS-інсталятор, у dev — `register_all()`) + `tauri-plugin-single-instance` (друга копія передає URL першій). Оболонка (`src-tauri/src/deeplink.rs`) лише перевіряє посилання й шле подію `deep-link` (стартове посилання чекає в черзі до `deeplink_pending`); UI (`src/deeplink.ts`) відкриває проєкт і вибирає етап/задачу. Команда `link_make` і кнопка «Копіювати посилання» в інспекторі (етап і задача).
- Б1/Б2/Б5 UI (зроблено): кольорова позначка стану сесії + підказка «стан змінено …» + кнопка відкриття в інспекторі етапу (на картці полотна не малюється); `ALREADY_DECIDED` розпізнається за текстом помилки ядра, UI показує «Вже вирішено: хто, коли» і оновлює вигляд; доказ показує «перевірено на `branch@sha7`».
- Б8: `--stage` не фільтрує `claims`, `board`, `proposal`, `agents`, `hello`.

**Б1. Живий стан сесії.** `SessionRef` отримує необов'язкові `state: working|waiting|blocked|error|done` і `state_at`. MCP `update_session {session, task_id?, stage?}` (upsert за `client`+`id`, оновлює `state`, додає в `sessions` етапу; без нового запису в журнал на кожну зміну стану, лише при зміні значення). Будь-який мутаційний виклик із `session` теж оновлює стан. Картка й інспектор: кольорова позначка стану + «Відкрити сесію» (`url`). Capability `session_state`.

**Б2. Спільна черга погоджень.** Наявні `request_approval` / `ask_decision` — це і є `approval.request`. Нове:
- Лише коли в `.pathloom/project.toml` `allow_client_answers = true` (типово вимкнено; інакше `FORBIDDEN`): MCP `respond_approval {task_id, approve, reasons?, comment?, human}` і `answer_decision {task_id, choice, human}` — для клієнта, який передає відповідь людини (автор `<client>:<human>`). Агент не відповідає сам за себе: `human` обов'язкове й ≠ ідентичності агента.
- Перша відповідь вирішує: повторна відповідь на вже вирішене → помилка `ALREADY_DECIDED {by, at, state}`; застосунок і Telegram показують «уже вирішено».
- Події `watch --json`: `{"type":"approval","task_id","change":"requested|decided","state","by","at"}`, `{"type":"decision","task_id","change":"asked|answered","choice","by","at"}` (обчислюються з різниці `task_info` у watcher). Capability `approval_events`.

**Б3. Повернення задачі.** Відхилення погодження (будь-ким) → подія `{"type":"task","task_id","change":"returned","comment","reasons","by","at"}`, щоб клієнт продовжив ту саму сесію. Capability `task_events`.

**Б5.** `Evidence.branch?` (≤ 200). Картка: «перевірено на `branch@sha7`».

**Б7. Посилання в обидва боки.** Схема `pathloom://open?project=<шлях>&stage=KEY&task=ID` (Tauri deep-link, ліцензія MIT/Apache) відкриває проєкт і картку. `pathloom link ID` друкує посилання. Із картки — `url` сесії (вже є).

**Б8. Фільтр подій.** `pathloom watch --json [--stage K]... [--type stage,approval,…]`; MCP-ресурс не змінюється. Capability `watch_filter`.

## А. Люди без коду

**А3. «Сьогодні / Тиждень».** Новий вид: прострочене й на термін сьогодні/цей тиждень (`due`), заблоковане, «чекає на мене» (inbox з m9). Ядро `Project::agenda(today, days) -> Agenda {overdue, due, blocked, waiting}`; CLI `pathloom today [--week] [--json]`.

**А4. Нагадування й підсумок.** Застосунок (системні сповіщення, `tauri-plugin-notification`, MIT/Apache): терміни (за 1 день і в день), нові елементи «чекає на мене»; щоденний підсумок у вибраний час (зроблено за добу з журналу, застрягло: заблоковане/мовчазні агенти/прострочене). CLI `pathloom digest [--md]` — той самий текст для плагінів (Telegram-міст надсилає його, якщо `[telegram] digest = "09:00"`). Налаштування в застосунку; нічого не надсилається без увімкнення.

**А1. Шаблони.** Вбудовані: «Запуск продукту», «Написати книгу», «Ремонт», «Навчальний курс» (apply-специфікації з етапами, задачами, залежностями, рамками; en/uk). `pathloom init --template NAME`, `pathloom template list|save NAME|apply NAME`; власні шаблони — у `~/.pathloom/templates/<name>.json` (зберігається план без журналу, захоплень, доказів і дат). Стартовий екран: «Новий з шаблону».

**А2. Швидке додавання простою мовою.** Правиловий розбір (без мережі й моделей): назва в лапках, етап за назвою/ключем (нечіткий збіг), термін («сьогодні», «завтра», «до п'ятниці», «12.12», ISO), виконавець («мені», «агенту»). Ядро `quickadd::parse(text, stages, today) -> Draft`; застосунок показує поля перед збереженням; CLI `pathloom add "…" [--yes]`. **Готово (core `quickadd.rs`, CLI `add`, MCP `parse_quick_add`).** Відхилення: `--yes` додає лише задачу; термін ставиться на етап, лише якщо в нього ще немає `due` (інакше примітка); «мені/агенту» показується, але не зберігається (у задач немає виконавця). `dd.mm` у минулому → наступний рік; день тижня — найближчий строго після сьогодні. Закінчення відмінків додатково `я`/`ї`.

**А5. Таймлайн з критичним шляхом.** Необов'язкове `estimate` (днів) в етапі (без нього — 1). Ядро: ранній старт/фініш від сьогодні за залежностями, запас відносно `due`, критичний шлях (нульовий запас до фінішу). Вид «Таймлайн»: смуги на осі часу, лінія «сьогодні», критичні — виділені; підказка «запізнення зсуне фініш на N днів». **Готово (core `schedule.rs`, `estimate` у frontmatter, `apply stages[].estimate`, CLI `schedule`, MCP `schedule`, capability `schedule`).** Лише жорсткі залежності; завершені етапи — 0 днів, не критичні; елементи мають додаткове `float`. UI: вид «Таймлайн» (`TimelineView.tsx`) і поле «Оцінка (днів)» в інспекторі.

**А6. Імпорт/експорт. ВИКОНАНО.** Експорт: Markdown, CSV, JSON для GitHub Issues (`gh`-сумісний скрипт без мережі з боку Pathloom). Імпорт: Markdown-списки, CSV, експорт Trello (JSON) і Notion (CSV/Markdown). Усе — через `apply` (dry-run/propose працюють).
Реалізація (`pathloom-core/src/interchange.rs`): `pathloom export md|csv|github|html [-o FILE]` (stdout за замовчуванням); `pathloom import md|csv|trello|notion|spec-kit FILE [--dry-run] [--propose] [--direct]`. MCP: `export_plan {format: md|csv|github|html}` (лише читання), `import_tasks {format, text, dry_run?, propose?}` (тепер ще md/csv/notion/trello). CSV-колонки: `stage_id,stage_title,task_id,task_title,done,who,priority,due,depends` (`depends` через `;`); також приймається `title`/`Name` + `Status` (done/complete = виконано) + група `Stage`/`Group`/`List`/`Section`/`Phase`, інакше один етап `Imported`. Markdown: `## KEY: Назва` зберігає ключ, рядки `Depends:`/`Due:`; вкладені пункти й чек-листи Trello стають звичайними задачами (apply не має підзадач). Ключі нових етапів — `generate_key` без колізій; етап з тією самою назвою перевикористовується (повторний імпорт додає лише нове). Можливості: `export` (також read-only), `import_formats`.

**А7. Історія з відкатом окремої зміни. ВИКОНАНО.** Кожна дія з журналу зберігає вміст змінених файлів до/після (`.pathloom/history/<id>.json`, ротація за розміром). «Відкотити» лише якщо файл досі дорівнює «після» (інакше — конфлікт). CLI `pathloom history [--limit N] [--json]`, `pathloom revert ID`; MCP `list_history {limit?}` (лише читання), `revert_change {id}`; можливість `history`.
Реалізація (`pathloom-core/src/history.rs`): хук — `Project::log_action` порівнює файли під `plan_dir` з базовою копією `.pathloom/cache/history-base.json` і пише запис, якщо щось змінилось (код викликачів не змінено). Обмеження: правки поза Pathloom між двома діями потрапляють у наступну дію; у проєкті без базової копії (створений до цієї функції) перша дія лише створює її; файли >2 МБ чи не-UTF-8 не відстежуються; ротація ~20 МБ. `revert` — конфлікт (`Error::HistoryConflict`, код `CONFLICT`, `details.paths`), якщо файл уже не дорівнює «після»; сам відкат логується як дія `revert` і теж відкочується. `.pathloom/history/` додається в `.gitignore` лише нових проєктів.

**А8. «Лише перегляд».** `pathloom export html` / кнопка в застосунку: один самодостатній HTML (дошка, етапи, прогрес) без скриптів зовнішніх доменів — файл можна надіслати або викласти будь-де. Хостингу Pathloom не робить. **Готово ядро+CLI (`export.rs`, `pathloom export html [-o]`, capability `export_html`); кнопки в застосунку немає.** Без JS; SVG дошки = типова дошка `roadmap` (або перша), прямі лінії між картками.

## Тести й документація
Кожна частина: тести ядра, CLI і MCP (включно із сумісністю: старі файли/клієнти без нових полів), оновлення цього документа, `capabilities`, `guide.md`, `SKILL.md`.

## Стан на 2026-11-20
Усе з А1–А8 і Б1–Б8 зроблено в ядрі, CLI, MCP і застосунку. UI застосунку: «Сьогодні / Тиждень», нагадування (типово вимкнені), шаблони, швидке додавання (Ctrl+N), «Таймлайн», експорт/імпорт і «Історія» з відкатом (меню проєкту), стан сесії й «Копіювати посилання» в інспекторі.
