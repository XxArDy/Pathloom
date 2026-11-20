# Погодження, рішення, живий статус, git — контракти (m9)

Два запити власника (2026-11-20), зведені в один контракт. Протокол лишається `pathloom/1`: усе нове — необов'язкові поля й нові tools/команди. Старі клієнти й старі файли працюють як раніше. Нові можливості в `capabilities` (у `--read-only` лише `commits`):

`propose`, `approvals`, `review`, `decisions`, `inbox`, `delegation`, `task_status`, `evidence_commit`, `commits`, `usage_tasks`, `usage_quota`.

Загальні правила: запис атомарний; нові поля frontmatter пишуться лише коли не порожні (`skip_serializing_if`), тож файл етапу без них не змінюється байт у байт; невалідні нові поля, відредаговані руками, ігноруються й не ламають етап (як `evidence`).

## 1. Пропозиції змін плану (v2)

Наявне: `.pathloom/proposals/<id>.json`, режим `agent_changes = "review"`, CLI `pathloom proposals list|accept|reject`.

- `Proposal` отримує поля:
  - `stage_hashes: {KEY: sha256}` — хеш файлу кожного **наявного** етапу, який пропозиція змінює (з `preview.updated`), на момент подачі;
  - `changes: [{path, before?, after?}]` — повний текст файлів етапів до/після (`before` відсутній для нового файлу). Дошки (`*.board.json`) у `changes` не входять;
  - `decision?: {state: approved|rejected|conflict, comment?, at, by}`.
- Подача: `apply {spec, propose: true}` (MCP) / `pathloom apply FILE --propose` (CLI) — зберігає пропозицію незалежно від `agent_changes`. Режим `review` як і раніше робить пропозицію за замовчуванням.
- Рішення: `Project::decide_proposal(id, approve, comment, by) -> Result<Proposal>`.
  - Схвалення: якщо поточний хеш будь-якого етапу з `stage_hashes` інший (або етап зник) — `state: conflict`, нічого не застосовується. Інакше `apply` від імені автора; успіх → `approved`.
  - Відхилення → `rejected` (коментар необов'язковий).
  - Вирішена пропозиція переїжджає в `.pathloom/proposals/decided/<id>.json` (зберігаються останні 200, старіші видаляються). Список «очікують» — лише файли верхнього рівня.
  - `accept_proposal`/`reject_proposal` лишаються як обгортки (CLI `proposals accept|reject` працює як раніше).
- MCP: `list_proposals {state?: pending|approved|rejected|conflict|all}` (типово `pending`; повертає `id, author, created_at, preview, changes, decision`), `decide_proposal {id, approve, comment?}`.
- CLI: `pathloom proposal list [--state S]`, `pathloom proposal approve ID [--comment]`, `pathloom proposal reject ID [--comment]` (`proposals` лишається аліасом, `accept` = `approve`).
- `pathloom watch --json`: `{"type":"proposal","id":"…","change":"created"}` і нове `{"type":"proposal","id":"…","change":"decided","state":"approved|rejected|conflict","at":"…"}`. Поле `state` необов'язкове (лише в `decided`). Перенесення файлу в `decided/` не дає події `deleted`. Назви із запиту: `proposal.created` = `type: proposal, change: created`; `proposal.decided` = `change: decided`.
- Застосунок: панель «Пропозиції» (лічильник у шапці, якщо > 0): автор, час, що змінюється, построковий дифф `before`/`after`, кнопки «Схвалити» / «Відхилити» (з необов'язковим коментарем); конфлікт показується повідомленням.

## 2. Погодження задач і стан «на перевірці»

Нові необов'язкові поля в `task_info.<ID>`:

```yaml
task_info:
  APPR-01:
    kind: approval            # approval | decision; необов'язково
    files: [shots/before.png, shots/after.png]
    note: Нова іконка
    approval: {state: pending, requested_by: claude, requested_at: 2026-11-20T10:00:00Z}
  APPR-02:
    approval: {state: rejected, reasons: [off-theme, quality], comment: Темніше, at: …, by: me, requested_by: claude, requested_at: …}
```

- Вид задачі: `kind` з `task_info`, інакше за ключем етапу: `APPR` → `approval`, `DECIDE` → `decision` (id задачі `APPR-03`, `DECIDE-01`). Ядро: `Stage::task_kind(id) -> Option<TaskKind>`.
- `approval.state`: `pending | approved | rejected`; `reasons` — до 10 рядків по ≤ 40 символів (стандартні id: `off-theme`, `duplicate`, `quality`, `scale`, `wrong-place`; дозволено й інші); `comment` ≤ 2000; `at`, `by`, `requested_by`, `requested_at`.
- «На перевірці» (`review`) = `approval.state == pending` і задача не виконана. Обчислюється, не зберігається окремо. Етап з такою задачею — `active`.
- Запит: `Project::request_approval(task_id, files, note, by)`: додає файли (дедуп, ліміти `task_info`), замінює нотатку якщо дано, ставить `approval = {state: pending, requested_by, requested_at}` (старі `reasons/comment` прибираються — вони вже є в коментарях етапу), знімає галочку. MCP `request_approval {task_id, files?, note?}`, CLI `pathloom approval request ID [--file F]... [--note T]`.
- Рішення власника: `Project::decide_approval(task_id, approve, reasons, comment, by)`:
  - approve → галочка ставиться, `state: approved`; якщо в задачі є `status` (§4) — `status.step = accepted`;
  - reject → галочка знята, `state: rejected`, `reasons`, `comment`; у коментарі етапу додається рядок `<by>: rejected <ID> [reasons]: <comment>`.
  - CLI `pathloom approval approve ID`, `pathloom approval reject ID [--reason R]... [--comment T]`; застосунок — кнопки на картці задачі. MCP-інструменту для рішення **немає** (агент не погоджує сам себе).
- Читання: MCP `get_approvals {state?}` → `[{task_id, stage, title, state, files, note, reasons, comment, at, by, requested_by, requested_at}]` по всіх етапах (лише задачі з `approval`). CLI `pathloom approval list [--state S]`. `get_stage` і так повертає `task_info`.
- Налаштування `.pathloom/project.toml`: `require_approval = true` (типово `false`). Тоді `set_task {done: true}` від агента (MCP, і `tasks_done` в `apply` від агента) **не** ставить галочку: задача переходить у `review` (`approval.state = pending`, `requested_by` = агент), доказ (`evidence`) зберігається. Відповідь MCP: `{"state": "review", …}`. Клацання людини в застосунку й CLI не обмежені. `prune_evidence` зберігає доказ задач у `review`.
- Повторна перевірка (необов'язково, лише кнопкою людини): у застосунку «Перевірити ще раз» біля доказу з `command` → підтвердження команди → запуск у корені проєкту → показ exit code (нічого не записує). Автоматичного запуску немає.

## 3. Рішення (`decision`)

```yaml
task_info:
  DECIDE-01:
    decision: {question: "Яку БД?", options: [{label: SQLite, description: локально}, {label: Postgres}], recommended: 0,
               choice: SQLite, at: …, by: me, asked_by: claude}
```

- MCP `ask_decision {task_id, question, options: [{label, description?}], recommended?}` → `Project::ask_decision(...)`: 1–8 варіантів, `label` ≤ 80, `description` ≤ 500, `question` ≤ 500, `recommended` — індекс; скидає попередній `choice`, знімає галочку. CLI `pathloom decision ask ID --question Q --option "label|description"... [--recommended N]`.
- Відповідь власника: `Project::decide(task_id, choice, by)` (`choice` — `label` варіанта або власний текст ≤ 2000): записує `choice, at, by`, ставить галочку, рядок у журнал. CLI `pathloom decision answer ID "текст"`; застосунок — кнопки варіантів (рекомендований виділено) + поле «свій варіант».
- Агент читає відповідь через `get_stage` (`task_info`) або подію `stage updated` у `watch`. MCP `list_decisions {open?: bool}`.
- Експорт: `pathloom decisions [--md]` — усі рішення з відповіддю; `--md` дає Markdown:
  ```
  ## DECIDE-01 — Яку БД?
  - Рішення: SQLite
  - Хто/коли: me, 2026-11-20
  - Варіанти: SQLite (рекомендовано), Postgres
  ```

## 4. Живий статус і делегування

- `Claim` (у `claims.json`) — нові необов'язкові поля: `by` (хто реально робить: модель/агент, ≤ 64), `parent` (id агента, що делегував), `branch` (≤ 200), `worktree` (≤ 500). MCP `claim_task {…, by?, parent?, branch?, worktree?}`, CLI `pathloom task claim ID [--by] [--parent] [--branch] [--worktree]`. `ClaimView` віддає їх (camelCase).
- «Застаріле» захоплення: без heartbeat > 30 хв (`STALE_AFTER_SECS = 1800`) → `ClaimView.stale = true` (окрім наявного `silent` після 10 хв).
- Статус задачі `task_info.<ID>.status`:
  ```yaml
  status: {step: pushed, branch: feat/x, sha: 3f2a9c1, evidence: [shots/x.png], at: …, by: claude}
  ```
  `step`: `claimed | pushed | merged | verified | accepted`; `sha` — 7–40 hex; `evidence` — до 10 шляхів/посилань (правила як у `files`). MCP `set_task_status {task_id, status, branch?, sha?, evidence?}`, CLI `pathloom task status ID STEP [--branch] [--sha] [--evidence F]...`. Виклик теж оновлює heartbeat захоплень агента.
- Застосунок: на картці задачі — агент (і ланцюжок «parent → by (branch)»), `branch@sha`, конвеєр з 5 кроків, мініатюри `status.evidence`; позначка «застаріле». У шапці — список активних агентів з їхньою поточною задачею.

## 5. Доказ, прив'язаний до коду, і коміти етапу

- `Evidence.commit?` — sha (7–40 hex). MCP `set_task {…, evidence: {…, commit}}`, CLI `pathloom task done ID --run … [--commit SHA]`.
- Застосунок: якщо `HEAD` репозиторію ≠ `commit` (порівняння за префіксом), щит показує «перевірено на <sha7>» замість «перевірено». Done не скидається.
- Коміти ↔ етапи: `pathloom_core::commits::scan(root, limit) -> Vec<CommitRef {sha, subject, author, at, stages, tasks}>` через `git log` (немає git / не репозиторій → порожньо). Префікс повідомлення до `:`:
  - `NATIVE-05: …` → задача `NATIVE-05`; `AUTH-02, 03: …` → `AUTH-02`, `AUTH-03` (номер доповнюється нулями до ширини першого);
  - `AUTH: …` → етап `AUTH`; також `AUTH-02, BILL-01: …`.
  - Ключ — `[A-Z][A-Z0-9]{0,9}`; враховуються лише ключі наявних етапів.
- CLI `pathloom commits [--stage K] [--limit N] [--json]`, MCP `list_commits {stage?, limit?}`. Застосунок: блок «Коміти» в інспекторі етапу.

## 6. Витрати по задачах і ліміт

- `Usage` — нові необов'язкові поля `model`, `percent_weekly` (0–100), `resets_at` (RFC3339). Токени можуть бути 0, якщо передано лише ліміт.
- MCP `report_usage {tokens_in?, tokens_out?, cost_usd?, stage?, task_id?, model?, percent_weekly?, resets_at?, note?}`; `task_id` додає задачу й її етап у запис журналу. CLI `pathloom usage --task ID --model M --percent-weekly P --resets-at T`.
- `UsageSummary`: `by_stage_model: {stage: {model: Usage}}` (модель без назви — `"?"`), `quota: {agent: {percent_weekly, resets_at, at}}` — останнє значення кожного агента.
- Застосунок: на етапі — сума токенів/вартості з розбивкою за моделлю; у шапці — віджет «ліміт N%» (найбільше значення серед агентів), жовтий від порогу (налаштування, типово 70 %).

## 7. Дошка з git-гілки

`.pathloom/project.toml`:
```toml
[git]
read_ref = "origin/main"      # немає секції/поля — вимкнено
fetch_interval_secs = 120
```

- Ядро `gitsrc`: `fetch(root)`; `materialize(root, ref, plan_dir) -> TempDir` — `git ls-tree -r --name-only <ref> -- <plan_dir>` + `git show <ref>:<path>` у тимчасову папку разом із локальним `.pathloom/project.toml`; `Project::open` на ній дає стани етапів з гілки. Захоплення, журнал, пропозиції — локальні.
- Застосунок: якщо `read_ref` задано — кожні `fetch_interval_secs` `git fetch`, а знімок проєкту (етапи, дошки) береться з `read_ref`; у шапці позначка «з origin/main @sha». Помилка git — показати й читати робочу копію.
- Дії власника (approve/reject/decide) пишуться в локальну копію: якщо дерево чисте — спершу `git merge --ff-only <read_ref>` (якщо можливо), запис, `git commit -m "owner: <action> <task>" -- <файл етапу>`. Якщо дерево брудне або ff неможливий — дія стає в чергу `.pathloom/owner-queue.jsonl` і повторюється, коли дерево чисте. `push` Pathloom не робить.

## 8. Telegram (необов'язково)

- `.pathloom/project.toml`: `[telegram] chat_id = "123"`; токен — лише змінна середовища `PATHLOOM_TELEGRAM_TOKEN`. Токен ніколи не пишеться в лог/stdout/помилки (помилки HTTP очищаються від нього).
- `pathloom telegram` — довгоживучий міст: кожен новий `approval.state = pending` (ще не надісланий, стан у `.pathloom/telegram.json`) → фото-превʼю (`files`, png/jpg/gif, локальні або URL) + повідомлення з кнопками ✅ / ❌. ❌ показує кнопки причин (`off-theme`, `duplicate`, `quality`, `scale`, `wrong-place`, «без причини»). Відповідь записується через `decide_approval` (by: `telegram:<username>`). Повідомлення від інших чатів ігноруються.

## 9. Застосунок: «Чекає на мене»

- `Project::inbox() -> Vec<InboxItem {task_id, stage, title, kind: approval|decision|owner}>`: задачі з `approval.state = pending`; рішення без `choice`; невиконані задачі етапів з тегом `owner` або `who: me`, які не `blocked`. CLI `pathloom inbox [--json]`.
- Шапка: лічильник-бейдж «Чекає на мене» (N); клік відкриває список (фільтр). Картка погодження: мініатюри `files` (png/jpg/gif/webp, локальні через `plasset` або URL), клік — повний розмір, пара з двох зображень — повзунок «до/після»; кнопки «Погодити» / «Відхилити» → чипи причин + коментар.
- Усі рядки — через i18n (`en`, `uk`).
