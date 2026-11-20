"""Builds site/index.html (EN) and site/uk/index.html (UK) from one template.

Run after editing copy or markup: `python site/build.py`. Both outputs are committed,
GitHub Pages serves them as static files. Strings live in `STR`; `{{key}}` in the
template is replaced by the string for the page language.
"""
import json
import pathlib
import re

ROOT = pathlib.Path(__file__).parent
REPO = "https://github.com/XxArDy/Pathloom"
SITE = "https://xxardy.github.io/Pathloom/"

# Icons: one sprite, 24x24 stroke icons (lucide-style geometry).
ICONS = {
    "done": '<circle cx="12" cy="12" r="9"/><path d="m8 12.5 2.7 2.7L16 10"/>',
    "active": '<circle cx="12" cy="12" r="9" stroke-dasharray="3.2 2.6"/>',
    "ready": '<path d="M8 5.5v13l10-6.5z"/>',
    "blocked": '<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>',
    "flag": '<path d="M5 21V4m0 0h11l-2 4 2 4H5"/>',
    "branch": '<circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="7" r="2"/><path d="M6 7v10m12-8a8 8 0 0 1-8 8H8"/>',
    "search": '<circle cx="11" cy="11" r="6.5"/><path d="m20 20-4.2-4.2"/>',
    "chev": '<path d="m6 9 6 6 6-6"/>',
    "map": '<path d="m3 6 6-3 6 3 6-3v15l-6 3-6-3-6 3z"/><path d="M9 3v15m6-12v15"/>',
    "list": '<path d="M9 6h11M9 12h11M9 18h11"/><path d="M4 6h1M4 12h1M4 18h1"/>',
    "overview": '<rect x="3" y="3" width="7" height="9" rx="1.5"/><rect x="14" y="3" width="7" height="5" rx="1.5"/><rect x="14" y="12" width="7" height="9" rx="1.5"/><rect x="3" y="16" width="7" height="5" rx="1.5"/>',
    "hand": '<path d="M8 13V5.5a1.5 1.5 0 0 1 3 0V12m0-1.5v-6a1.5 1.5 0 0 1 3 0V12m0-5.5a1.5 1.5 0 0 1 3 0V13m0-3.5a1.5 1.5 0 0 1 3 0V15a6 6 0 0 1-6 6h-1.5a6 6 0 0 1-5-2.7L4 15a1.6 1.6 0 0 1 2.6-1.8L8 15"/>',
    "pointer": '<path d="m4 4 7 17 2.5-7.5L21 11z"/>',
    "square": '<rect x="4" y="4" width="16" height="16" rx="2"/>',
    "diamond": '<path d="M12 3 21 12 12 21 3 12z"/>',
    "circle": '<circle cx="12" cy="12" r="8.5"/>',
    "arrow": '<path d="M6 18 18 6m-9 0h9v9"/>',
    "pencil": '<path d="M17 3.5a2.1 2.1 0 0 1 3 3L8 18.5 4 20l1.5-4z"/>',
    "text": '<path d="M5 7V5h14v2M12 5v14m-3 0h6"/>',
    "sticky": '<path d="M5 4h14v10l-6 6H5z"/><path d="M13 20v-6h6"/>',
    "frame": '<path d="M8 3v18M16 3v18M3 8h18M3 16h18"/>',
    "minus": '<path d="M5 12h14"/>',
    "plus": '<path d="M12 5v14M5 12h14"/>',
    "fit": '<path d="M4 9V5h4m8 0h4v4m0 6v4h-4m-8 0H4v-4"/>',
    "undo": '<path d="M9 14 4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 0 12h-3"/>',
    "redo": '<path d="m15 14 5-5-5-5"/><path d="M20 9H10a6 6 0 0 0 0 12h3"/>',
    "focus": '<circle cx="12" cy="12" r="3"/><path d="M12 2v3m0 14v3M2 12h3m14 0h3"/>',
    "x": '<path d="M7 7l10 10M17 7 7 17"/>',
    "github": '<path d="M9 19c-4.3 1.4-4.3-2.5-6-3m12 5v-3.5c0-1 .1-1.4-.5-2 2.8-.3 5.5-1.4 5.5-6a4.6 4.6 0 0 0-1.3-3.2 4.2 4.2 0 0 0-.1-3.2s-1.1-.3-3.5 1.3a12.3 12.3 0 0 0-6.2 0C6.5 2.8 5.4 3.1 5.4 3.1a4.2 4.2 0 0 0-.1 3.2A4.6 4.6 0 0 0 4 9.5c0 4.6 2.7 5.7 5.5 6-.6.6-.6 1.2-.5 2V21"/>',
    "download": '<path d="M12 4v11m-5-5 5 5 5-5M5 20h14"/>',
    "file": '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>',
    "check": '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
}


def icon(name, cls="ico"):
    return f'<svg class="{cls}" viewBox="0 0 24 24" aria-hidden="true"><use href="#i-{name}"/></svg>'


def sprite():
    syms = "".join(f'<symbol id="i-{k}" viewBox="0 0 24 24">{v}</symbol>' for k, v in ICONS.items())
    return f'<svg width="0" height="0" style="position:absolute" aria-hidden="true">{syms}</svg>'


CHK = '<span class="chk"><svg viewBox="0 0 10 10"><path d="M2.2 5.2 4.2 7.2 7.8 3"/></svg></span>'


def card(key, who, status, title, tasks, extra=""):
    """A static stage card in the app's anatomy (header, title, progress, tasks)."""
    done = sum(1 for t in tasks if t[2])
    rows = "".join(
        f'<li class="{"done" if d else ""}">{CHK}<span class="tid">{tid}</span><span class="tt">{tt}</span></li>'
        for tid, tt, d in tasks
    )
    pct = round(100 * done / len(tasks)) if tasks else 0
    who_html = f'<span class="who{" ai" if who == "AI" else ""}">{who}</span>' if who else ""
    return (
        f'<div class="card"><div class="ch"><span class="key">{key}</span>{who_html}'
        f'<span class="pill {status[0]}">{icon(status[0])}{status[1]}</span></div>'
        f'<div class="ct">{title}</div>{extra}'
        f'<div class="cp"><i style="--p:{pct}%"></i><span>{done}/{len(tasks)}</span></div>'
        f'<ul class="tasks">{rows}</ul></div>'
    )


STR = {
    "en": {
        "lang": "en", "locale": "en_US", "root": "", "url": SITE,
        "title": "Pathloom - the plan your AI agents keep up to date",
        "desc": "Pathloom is a local desktop app: an infinite whiteboard plus a roadmap of stage cards and dependencies. Your AI agent builds and updates the plan through MCP or the CLI while you watch it change live.",
        "skip": "Skip to content", "nav": "Main", "how": "How it works", "features": "Features", "cli": "CLI", "download": "Download",
        "langs": '<span aria-current="true">EN</span><a href="uk/" hreflang="uk" lang="uk">УК</a>',
        "h1": "The plan your <em>AI agents</em> keep up to date",
        "lead": "Pathloom is a local whiteboard and roadmap. Your agent builds the plan through MCP or the CLI. You watch the board change live, then approve or revert.",
        "dl_btn": "Download", "gh_btn": "View on GitHub", "also": "Windows · macOS · Linux",
        "dl_os": "Download for {os}", "pre": "pre-release", "also_for": "Also for",
        "mock_alt": "Animated Pathloom window. An agent called claude ticks the last tasks of the Core and data stage, the stage turns done, Connect an AI agent becomes ready to start, and the agent proposes a new stage for review.",
        "how_h": "From a prompt to a plan you can see",
        "how_sub": "The agent does the typing. You keep the overview and the last word.",
        "flow_alt": "Three stage cards linked by arrows: the agent plans, you watch it live, you approve or revert.",
        "flow": [
            ("PLAN", "AI", ("done", "Done"), "The agent plans", "Claude Code, Codex, Cursor or any MCP client reads the plan and works through it.",
             [("PLAN-01", "Reads the plan and claims a task", True), ("PLAN-02", "Adds stages and dependencies", True), ("PLAN-03", "Ticks work off as it goes", True)]),
            ("WATCH", "ME", ("active", "In progress"), "You watch it live", "The board updates the moment a file changes.",
             [("WATCH-01", "Cards show who works on what", True), ("WATCH-02", "Every change is highlighted", False), ("WATCH-03", "Focus mode shows what is next", False)]),
            ("DECIDE", "ME", ("ready", "Ready to start"), "You approve or revert", "The plan is plain Markdown and JSON in your repo, so it goes through git like code.",
             [("DECIDE-01", "Review proposals as a diff", False), ("DECIDE-02", "Undo any change in one click", False), ("DECIDE-03", "Answer from Telegram if you are away", False)]),
        ],
        "feat_h": "One board for the picture and the plan",
        "feat_sub": "Draw freely, then turn the sketch into stages with real dependencies.",
        "tabs": ["Board", "Roadmap", "Proposals", "Approvals", "Today", "Import and history"],
        "p_board": ("An infinite whiteboard that knows your plan", "Sketch the way you think, and put real stage cards on the same canvas.",
                    ["Shapes, arrows that stick to cards, pencil, sticky notes, images and frames.", "Clean or hand-drawn style, snap to grid, smart guides, minimap.", "Focus mode hides finished work and highlights what you can do now."],
                    "Pathloom board with phases, stage cards and dependency arrows"),
        "p_road": ("Stage cards with real dependencies", "Every stage has tasks with IDs like AUTH-03. Status is computed from the graph, never typed in.",
                   ["Ready, in progress, blocked or done, worked out from dependencies and tasks.", "Hard and soft dependencies, with cycle protection.", "Phases with progress, milestones and plan templates.", "Auto-arrange keeps the board tidy after every agent change."],
                   "Pathloom in focus mode: finished stages hidden, the next stages highlighted"),
        "p_agents": ("Agents propose, you decide", "In review mode an agent&rsquo;s changes arrive as dashed cards with a diff. Nothing lands until you say so.",
                     ["Approve or reject each proposal, with a reason the agent can read.", "One-click undo of anything an agent changed.", "Claims show which agent works on which task, live.", "Orchestrator protocol: session state, a shared approval queue, an events stream and <code>pathloom://</code> deep links."],
                     "Pathloom proposal review with a diff of the agent&rsquo;s changes"),
        "p_inbox": ("Everything that needs you, in one place", "Agents can ask you to approve a task or to choose between options. All of it collects in <b>Waiting for me</b>.",
                    ["Answer in the app, or from Telegram with <code>pathloom telegram</code>.", "Optionally, agents cannot close tasks and must send them to review.", "Decisions are recorded, so an agent does not ask twice."]),
        "inbox_bar": "Waiting for me",
        "inbox_alt": "Waiting for me inbox with an approval request, a choice between two options, and the same approval in Telegram",
        "inbox_rows": [("AUTH-04", "<b>claude wants to close this task</b> · Password reset", "Approve", "Reject"),
                       ("DB-01", "<b>Choose a database</b> · SQLite or Postgres", "SQLite", "Postgres")],
        "tg": "claude asks: close AUTH-04 &ldquo;Password reset&rdquo;?", "approve": "Approve", "reject": "Reject",
        "p_today": ("What to do today, and what is on the critical path", "Overview, Today and Week views show what is overdue, due and blocked, so you open the app and know.",
                    ["Quick add in plain words: type &ldquo;fix login bug friday&rdquo; and it becomes a task.", "Reminders and a daily digest.", "Timeline with the critical path highlighted."],
                    "Pathloom overview with progress, what is next, blockers and deadlines"),
        "p_io": ("Bring your plan in, take it out, undo anything", "Your plan is never locked in. Every change is logged and can be reverted.",
                 ["Import from Markdown, CSV, Trello or Notion.", "Export to a read-only HTML page, Markdown, CSV or GitHub issues.", "Change history with one-click revert."]),
        "hist_bar": "Change history", "hist_alt": "Change history with revert buttons, plus the import and export formats",
        "hist_rows": [("#41", "<b>claude</b> added 3 tasks to API"), ("#40", "<b>you</b> moved UI after API"), ("#39", "<b>claude</b> ticked AUTH-02")],
        "revert": "Revert", "import": "Import", "export": "Export",
        "cli_h": "Built for agents, readable by you",
        "cli_sub": "Every action in the app is also a command. The same plan is available through the CLI and a stdio MCP server.",
        "works": "Works with", "any": "any MCP client",
        "term_alt": "Terminal with Pathloom commands: init, MCP setup, next, task done, today, inbox, history, revert and export",
        "c_setup": "# set up once", "c_agent": "# what the agent does", "c_you": "# what you can do",
        "dl_h": "Download Pathloom",
        "dl_sub": "A desktop app for Windows, macOS and Linux. No account, no cloud: everything stays in your project folder.",
        "yours": "Your system",
        "os": [("WIN", "win", "Windows", [("WIN-01", "Installer (.exe)"), ("WIN-02", "MSI package")]),
               ("MAC", "mac", "macOS", [("MAC-01", "Disk image (.dmg)"), ("MAC-02", "Apple silicon and Intel")]),
               ("LINUX", "linux", "Linux", [("LINUX-01", "AppImage"), ("LINUX-02", "Debian package (.deb)")])],
        "dl_note": "This is an early release. Builds are not code-signed yet, so your system may ask you to confirm the first launch.",
        "all_rel": "All releases",
        "license": "MIT or Apache-2.0", "docs": "Docs", "changelog": "Changelog", "releases": "Releases", "saved": "Saved to files",
        "foot_nav": "Project links",
        "mock": {
            "me": "ME", "project": "my-app", "board": "roadmap", "search": "Search stages and tasks", "boards": "Boards", "onBoard": "Stages on this board",
            "all": "All", "active": "Active", "doneF": "Done", "focus": "Focus", "views": ["Board", "List", "Overview"],
            "status": {"done": "Done", "active": "In progress", "ready": "Ready to start", "blocked": "Blocked"},
            "ready": "{n} tasks ready", "blockedN": "{n} stages blocked", "saved": "Saved to files",
            "ofStages": "{d} of {n} stages", "inputs": "{d} of {n} inputs", "high": "High",
            "notice": "<b>claude</b> proposes 1 new stage", "review": "Review",
            "phases": ["Idea", "Foundation", "Product"],
            "stages": {
                "IDEA": ["Idea and requirements", ["Describe what the app does", "Collect references", "Pick the stack"]],
                "CORE": ["Core and data", ["Data model", "Save to files", "Core tests"]],
                "DESIGN": ["Screen design", ["Main screen mockup", "Dark theme", "App icon"]],
                "AGENT": ["Connect an AI agent", ["MCP server", "Agent instructions"]],
                "UI": ["User interface", ["Main screen", "Settings", "Keyboard shortcuts"]],
                "DOCS": ["README and screenshots", ["README", "Screenshots"]],
                "BETA": ["Beta release"],
            },
        },
    },
    "uk": {
        "lang": "uk", "locale": "uk_UA", "root": "../", "url": SITE + "uk/",
        "title": "Pathloom - план, який AI-агенти тримають актуальним",
        "desc": "Pathloom - локальний десктопний застосунок: нескінченна дошка плюс roadmap із карток етапів і залежностей. AI-агент будує й оновлює план через MCP або CLI, а ви бачите зміни наживо.",
        "skip": "Перейти до вмісту", "nav": "Головна навігація", "how": "Як це працює", "features": "Можливості", "cli": "CLI", "download": "Завантажити",
        "langs": '<a href="../" hreflang="en" lang="en">EN</a><span aria-current="true">УК</span>',
        "h1": "План, який <em>AI-агенти</em> тримають актуальним",
        "lead": "Pathloom - локальна дошка й roadmap. Агент будує план через MCP або CLI. Ви бачите, як дошка змінюється наживо, і схвалюєте або відкочуєте зміни.",
        "dl_btn": "Завантажити", "gh_btn": "Відкрити на GitHub", "also": "Windows · macOS · Linux",
        "dl_os": "Завантажити для {os}", "pre": "pre-release", "also_for": "Також для",
        "mock_alt": "Анімоване вікно Pathloom. Агент claude відмічає останні задачі етапу «Ядро і дані», етап стає готовим, «Підключити AI-агента» стає доступним, а агент пропонує новий етап на рев'ю.",
        "how_h": "Від запиту до плану, який видно",
        "how_sub": "Друкує агент. Загальна картина й останнє слово лишаються за вами.",
        "flow_alt": "Три картки етапів, з'єднані стрілками: агент планує, ви бачите наживо, ви схвалюєте або відкочуєте.",
        "flow": [
            ("PLAN", "AI", ("done", "Готово"), "Агент планує", "Claude Code, Codex, Cursor чи будь-який MCP-клієнт читає план і виконує його.",
             [("PLAN-01", "Читає план і бере задачу", True), ("PLAN-02", "Додає етапи й залежності", True), ("PLAN-03", "Відмічає виконане по ходу", True)]),
            ("WATCH", "Я", ("active", "В роботі"), "Ви бачите наживо", "Дошка оновлюється в ту ж мить, як змінюється файл.",
             [("WATCH-01", "Картки показують, хто над чим працює", True), ("WATCH-02", "Кожна зміна підсвічується", False), ("WATCH-03", "Режим «Фокус» показує, що далі", False)]),
            ("DECIDE", "Я", ("ready", "Можна починати"), "Ви схвалюєте або відкочуєте", "План - це звичайні Markdown і JSON у вашому репозиторії, тож він проходить через git як код.",
             [("DECIDE-01", "Переглядайте пропозиції як diff", False), ("DECIDE-02", "Скасовуйте будь-яку зміну одним кліком", False), ("DECIDE-03", "Відповідайте з Telegram, коли ви не за комп'ютером", False)]),
        ],
        "feat_h": "Одна дошка і для картини, і для плану",
        "feat_sub": "Малюйте вільно, а потім перетворюйте начерк на етапи зі справжніми залежностями.",
        "tabs": ["Дошка", "Roadmap", "Пропозиції", "Погодження", "Сьогодні", "Імпорт та історія"],
        "p_board": ("Нескінченна дошка, яка знає ваш план", "Малюйте так, як думаєте, і ставте справжні картки етапів на те саме полотно.",
                    ["Фігури, стрілки, що прилипають до карток, олівець, стікери, зображення й frames.", "Чистий або рукописний стиль, прив'язка до сітки, напрямні, мінікарта.", "Режим «Фокус» ховає завершене й підсвічує те, що можна робити зараз."],
                    "Дошка Pathloom з фазами, картками етапів і стрілками залежностей"),
        "p_road": ("Картки етапів зі справжніми залежностями", "Кожен етап має задачі з ID на кшталт AUTH-03. Статус обчислюється з графа, його не вписують вручну.",
                   ["Можна починати, в роботі, заблоковано чи готово - з залежностей і задач.", "Жорсткі й м'які залежності із захистом від циклів.", "Фази з прогресом, віхи й шаблони планів.", "Авторозкладка тримає дошку охайною після кожної зміни агента."],
                   "Pathloom у режимі «Фокус»: завершені етапи сховано, наступні підсвічено"),
        "p_agents": ("Агенти пропонують, ви вирішуєте", "У режимі рев'ю зміни агента приходять пунктирними картками з diff. Нічого не застосується, поки ви не погодите.",
                     ["Схвалюйте або відхиляйте кожну пропозицію, з причиною, яку агент прочитає.", "Скасування будь-якої зміни агента одним кліком.", "Claim наживо показує, який агент працює над якою задачею.", "Протокол для оркестраторів: стан сесії, спільна черга погоджень, потік подій і deep links <code>pathloom://</code>."],
                     "Рев'ю пропозиції в Pathloom з diff змін агента"),
        "p_inbox": ("Усе, що чекає на вас, в одному місці", "Агенти можуть попросити погодити задачу або обрати між варіантами. Усе збирається в розділі <b>Чекає на мене</b>.",
                    ["Відповідайте в застосунку або з Telegram через <code>pathloom telegram</code>.", "За бажанням агентам заборонено закривати задачі: вони надсилають їх на рев'ю.", "Рішення зберігаються, тож агент не питає двічі."]),
        "inbox_bar": "Чекає на мене",
        "inbox_alt": "Розділ «Чекає на мене»: запит на погодження, вибір із двох варіантів і те саме погодження в Telegram",
        "inbox_rows": [("AUTH-04", "<b>claude хоче закрити цю задачу</b> · Скидання пароля", "Схвалити", "Відхилити"),
                       ("DB-01", "<b>Оберіть базу даних</b> · SQLite чи Postgres", "SQLite", "Postgres")],
        "tg": "claude питає: закрити AUTH-04 «Скидання пароля»?", "approve": "Схвалити", "reject": "Відхилити",
        "p_today": ("Що робити сьогодні і що на критичному шляху", "Огляд, «Сьогодні» і «Тиждень» показують прострочене, термінове й заблоковане, тож ви відкриваєте застосунок і все знаєте.",
                    ["Швидке додавання звичайними словами: напишіть «виправити логін у п'ятницю», і це стане задачею.", "Нагадування й щоденний дайджест.", "Timeline з підсвіченим критичним шляхом."],
                    "Огляд Pathloom: прогрес, що далі, блокери й дедлайни"),
        "p_io": ("Імпортуйте, експортуйте, скасовуйте будь-що", "План ніколи не замкнений у застосунку. Кожну зміну записано, і її можна відкотити.",
                 ["Імпорт із Markdown, CSV, Trello або Notion.", "Експорт у HTML-сторінку для читання, Markdown, CSV або GitHub issues.", "Історія змін з відкотом в один клік."]),
        "hist_bar": "Історія змін", "hist_alt": "Історія змін із кнопками відкоту та формати імпорту й експорту",
        "hist_rows": [("#41", "<b>claude</b> додав 3 задачі до API"), ("#40", "<b>ви</b> перенесли UI після API"), ("#39", "<b>claude</b> відмітив AUTH-02")],
        "revert": "Відкотити", "import": "Імпорт", "export": "Експорт",
        "cli_h": "Зроблено для агентів, зрозуміло вам",
        "cli_sub": "Кожна дія в застосунку - це також команда. Той самий план доступний через CLI і stdio MCP-сервер.",
        "works": "Працює з", "any": "будь-який MCP-клієнт",
        "term_alt": "Термінал із командами Pathloom: init, налаштування MCP, next, task done, today, inbox, history, revert і export",
        "c_setup": "# налаштування один раз", "c_agent": "# що робить агент", "c_you": "# що можете ви",
        "dl_h": "Завантажте Pathloom",
        "dl_sub": "Десктопний застосунок для Windows, macOS і Linux. Без акаунта й хмари: усе лишається в папці вашого проєкту.",
        "yours": "Ваша система",
        "os": [("WIN", "win", "Windows", [("WIN-01", "Інсталятор (.exe)"), ("WIN-02", "Пакет MSI")]),
               ("MAC", "mac", "macOS", [("MAC-01", "Образ диска (.dmg)"), ("MAC-02", "Apple silicon та Intel")]),
               ("LINUX", "linux", "Linux", [("LINUX-01", "AppImage"), ("LINUX-02", "Пакет Debian (.deb)")])],
        "dl_note": "Це рання версія. Збірки ще не підписані, тож система може попросити підтвердити перший запуск.",
        "all_rel": "Усі релізи",
        "license": "MIT або Apache-2.0", "docs": "Документація", "changelog": "Список змін", "releases": "Релізи", "saved": "Збережено у файли",
        "foot_nav": "Посилання проєкту",
        "mock": {
            "me": "Я", "project": "my-app", "board": "roadmap", "search": "Пошук етапів і задач", "boards": "Дошки", "onBoard": "Етапи на цій дошці",
            "all": "Усі", "active": "Активні", "doneF": "Готові", "focus": "Фокус", "views": ["Дошка", "Список", "Огляд"],
            "status": {"done": "Готово", "active": "В роботі", "ready": "Можна починати", "blocked": "Заблоковано"},
            "ready": "Можна робити: {n}", "blockedN": "Заблоковано етапів: {n}", "saved": "Збережено у файли",
            "ofStages": "{d} з {n} етапів", "inputs": "{d} з {n} входів", "high": "Високий",
            "notice": "<b>claude</b> пропонує новий етап", "review": "Переглянути",
            "phases": ["Ідея", "Основа", "Продукт"],
            "stages": {
                "IDEA": ["Ідея і вимоги", ["Описати, що робить застосунок", "Зібрати референси", "Обрати стек"]],
                "CORE": ["Ядро і дані", ["Модель даних", "Збереження у файли", "Тести ядра"]],
                "DESIGN": ["Дизайн екранів", ["Макет головного екрана", "Темна тема", "Іконка"]],
                "AGENT": ["Підключити AI-агента", ["MCP-сервер", "Інструкції для агента"]],
                "UI": ["Інтерфейс", ["Головний екран", "Налаштування", "Гарячі клавіші"]],
                "DOCS": ["README і скріншоти", ["README", "Скріншоти"]],
                "BETA": ["Бета-реліз"],
            },
        },
    },
}

TEMPLATE = """<!doctype html>
<html lang="{{lang}}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{{title}}</title>
<meta name="description" content="{{desc}}">
<link rel="canonical" href="{{url}}">
<link rel="alternate" hreflang="en" href="SITE">
<link rel="alternate" hreflang="uk" href="SITEuk/">
<link rel="alternate" hreflang="x-default" href="SITE">
<meta property="og:type" content="website">
<meta property="og:title" content="{{title}}">
<meta property="og:description" content="{{desc}}">
<meta property="og:url" content="{{url}}">
<meta property="og:image" content="SITEassets/social-preview.png">
<meta property="og:locale" content="{{locale}}">
<meta name="twitter:card" content="summary_large_image">
<meta name="theme-color" content="#23262b">
<link rel="icon" type="image/png" href="{{root}}assets/favicon.png">
<link rel="preload" href="{{root}}assets/fonts/inter-latin-600-normal.woff2" as="font" type="font/woff2" crossorigin>
<link rel="stylesheet" href="{{root}}assets/site.css">
</head>
<body data-dl="{{dl_os}}" data-pre="{{pre}}" data-also="{{also_for}}">
SPRITE
<a class="skip" href="#main">{{skip}}</a>
<header class="top"><div class="wrap">
  <a class="brand" href="{{root}}{{langpath}}"><img src="{{root}}assets/icon.png" width="24" height="24" alt="">Pathloom</a>
  <nav aria-label="{{nav}}"><ul class="tabsnav">
    <li><a href="#how">{{how}}</a></li><li><a href="#features">{{features}}</a></li><li><a href="#cli">{{cli}}</a></li><li><a href="#download">{{download}}</a></li>
  </ul></nav>
  <span class="sp"></span>
  <div class="right">
    <a class="gh" href="REPO">I_GITHUB<span>GitHub</span></a>
    <div class="seg" role="group" aria-label="Language">{{langs}}</div>
    <a class="btn pri sm" href="#download">{{download}}</a>
  </div>
</div></header>

<main id="main">
<div class="hero"><div class="wrap">
  <h1>{{h1}}</h1>
  <p class="lead">{{lead}}</p>
  <div class="cta">
    <a class="btn pri" id="dl-main" href="REPO/releases" data-os="win">I_DOWNLOAD<span>{{dl_btn}}</span></a>
    <a class="btn" href="REPO">I_GITHUB<span>{{gh_btn}}</span></a>
  </div>
  <p class="also" id="also"><span data-ver>v0.1.0 {{pre}}</span> · {{also}}</p>

  <div class="win" role="img" aria-label="{{mock_alt}}">
    <div class="win-fit" id="win" aria-hidden="true"></div>
  </div>
</div></div>

<section id="how"><div class="wrap">
  <div class="sh"><h2>{{how_h}}</h2></div>
  <p class="sub">{{how_sub}}</p>
  <div class="flow" role="img" aria-label="{{flow_alt}}"><div class="flow-row" id="flow" aria-hidden="true">FLOW_CARDS</div></div>
</div></section>

<section id="features"><div class="wrap">
  <div class="sh"><h2>{{feat_h}}</h2></div>
  <p class="sub">{{feat_sub}}</p>
  <div class="views" role="tablist" aria-label="{{feat_h}}">TABS</div>
  PANELS
</div></section>

<section id="cli"><div class="wrap cli">
  <div>
    <div class="sh"><h2>{{cli_h}}</h2></div>
    <p class="sub">{{cli_sub}}</p>
    <p class="lbl" style="margin:28px 0 0">{{works}}</p>
    <div class="clients" style="margin-top:10px"><span>Claude Code</span><span>Codex</span><span>Cursor</span><span>Gemini CLI</span><span>VS Code Copilot</span><span>Claude Desktop</span><span class="any">{{any}}</span></div>
  </div>
  <div class="term" role="img" aria-label="{{term_alt}}"><div class="bar"><i></i><i></i><i></i><span>~/my-app</span></div>
<pre aria-hidden="true"><span class="cm">{{c_setup}}</span>
<span class="pr">$</span> pathloom init --name "my-app"
<span class="pr">$</span> claude mcp add pathloom -- pathloom mcp

<span class="cm">{{c_agent}}</span>
<span class="pr">$</span> pathloom next
<span class="pr">$</span> pathloom task done CORE-02
<span class="out">✓</span> CORE-02 done · CORE 2/3
<span class="pr">$</span> pathloom inbox

<span class="cm">{{c_you}}</span>
<span class="pr">$</span> pathloom today --week
<span class="pr">$</span> pathloom history
<span class="pr">$</span> pathloom revert &lt;id&gt;
<span class="pr">$</span> pathloom export html -o plan.html</pre></div>
</div></section>

<section id="download"><div class="wrap">
  <div class="sh"><h2>{{dl_h}}</h2></div>
  <p class="sub">{{dl_sub}}</p>
  <div class="dls">DL_CARDS</div>
  <p class="dl-note"><span data-ver>v0.1.0 {{pre}}</span>. {{dl_note}} <a href="REPO/releases">{{all_rel}}</a>.</p>
</div></section>
</main>

<footer><div class="wrap">
  <a class="brand" href="{{root}}{{langpath}}"><img src="{{root}}assets/icon.png" width="20" height="20" alt="">Pathloom</a>
  <span>{{license}}</span>
  <nav aria-label="{{foot_nav}}">
    <a href="REPO">GitHub</a><a href="REPO/blob/main/docs/README.md">{{docs}}</a><a href="REPO/blob/main/CHANGELOG.md">{{changelog}}</a><a href="REPO/releases">{{releases}}</a>
  </nav>
  <span class="saved">I_CHECK{{saved}}</span>
</div></footer>
<script id="mock-data" type="application/json">MOCK</script>
<script src="{{root}}assets/site.js"></script>
</body>
</html>
"""

SHOTS = {"p_board": "board", "p_road": "focus", "p_agents": "proposal", "p_today": "overview"}
TAB_ICONS = ["map", "branch", "sticky", "check", "overview", "undo"]


def panels(s):
    out, tabs = [], []
    keys = ["p_board", "p_road", "p_agents", "p_inbox", "p_today", "p_io"]
    for i, (k, label) in enumerate(zip(keys, s["tabs"])):
        sel = "true" if i == 0 else "false"
        ti = "" if i == 0 else ' tabindex="-1"'
        tabs.append(f'<button role="tab" id="t-{k}" aria-controls="{k}" aria-selected="{sel}"{ti}>{icon(TAB_ICONS[i])}{label}</button>')
        h, p, items = s[k][0], s[k][1], s[k][2]
        lis = "".join(f"<li>{x}</li>" for x in items)
        if k in SHOTS:
            img = f'{s["root"]}assets/img/{s["lang"]}/{SHOTS[k]}.png'
            media = f'<div class="shot"><img src="{img}" width="1440" height="900" loading="lazy" alt="{s[k][3]}"></div>'
        elif k == "p_inbox":
            rows = "".join(f'<div class="lrow"><span class="k">{a}</span><span class="g">{b}</span><span class="mb ok">{c}</span><span class="mb">{d}</span></div>' for a, b, c, d in s["inbox_rows"])
            media = (f'<div class="shot" role="img" aria-label="{s["inbox_alt"]}"><div class="bar"><b>{s["inbox_bar"]}</b></div>'
                     f'<div class="list" aria-hidden="true">{rows}<div class="tg">{s["tg"]}<br><span class="mb">{s["approve"]}</span><span class="mb">{s["reject"]}</span></div></div></div>')
        else:
            rows = "".join(f'<div class="lrow"><span class="k">{a}</span><span class="g">{b}</span><span class="mb">{s["revert"]}</span></div>' for a, b in s["hist_rows"])
            chips = lambda xs: "".join(f'<span class="chip">{x}</span>' for x in xs)
            media = (f'<div class="shot" role="img" aria-label="{s["hist_alt"]}"><div class="bar"><b>{s["hist_bar"]}</b></div>'
                     f'<div class="list" aria-hidden="true">{rows}<p class="lbl">{s["import"]}</p><div class="chips">{chips(["Markdown", "CSV", "Trello", "Notion"])}</div>'
                     f'<p class="lbl">{s["export"]}</p><div class="chips">{chips(["HTML", "Markdown", "CSV", "GitHub issues"])}</div></div></div>')
        hidden = "" if i == 0 else " hidden"
        out.append(f'<div class="fpanel" id="{k}" role="tabpanel" aria-labelledby="t-{k}"{hidden}><div><h3>{h}</h3><p>{p}</p><ul>{lis}</ul></div>{media}</div>')
    return "".join(tabs), "\n  ".join(out)


def dl_cards(s):
    out = []
    for key, os_id, name, files in s["os"]:
        rows = "".join(f'<li>{icon("file")}<span class="tid">{tid}</span><span class="tt">{tt}</span></li>' for tid, tt in files)
        out.append(
            f'<div class="card" data-card="{os_id}"><div class="ch"><span class="key">{key}</span>'
            f'<span class="pill ready">{icon("ready")}{s["yours"]}</span></div><div class="ct">{name}</div>'
            f'<ul class="tasks">{rows}</ul><a class="btn pri" data-os="{os_id}" href="{REPO}/releases">{icon("download")}<span>{s["dl_btn"]}</span></a></div>'
        )
    return "".join(out)


def flow_cards(s):
    return "".join(card(k, w, st, t, tasks, f"<p>{p}</p>") for k, w, st, t, p, tasks in s["flow"])


def build(lang):
    s = STR[lang]
    tabs, pans = panels(s)
    html = TEMPLATE
    html = html.replace("SPRITE", sprite()).replace("FLOW_CARDS", flow_cards(s)).replace("TABS", tabs)
    html = html.replace("PANELS", pans).replace("DL_CARDS", dl_cards(s))
    html = html.replace("I_GITHUB", icon("github")).replace("I_DOWNLOAD", icon("download")).replace("I_CHECK", icon("check"))
    html = html.replace("MOCK", json.dumps(s["mock"], ensure_ascii=False).replace("</", "<\\/"))
    html = html.replace("{{langpath}}", "" if lang == "en" else "uk/")
    html = re.sub(r"\{\{(\w+)\}\}", lambda m: s[m.group(1)], html)
    html = html.replace("REPO", REPO).replace("SITE", SITE)
    out = ROOT / ("index.html" if lang == "en" else "uk/index.html")
    out.parent.mkdir(exist_ok=True)
    out.write_text(html, encoding="utf-8", newline="\n")
    print("wrote", out.relative_to(ROOT.parent))


if __name__ == "__main__":
    for lang in STR:
        build(lang)
