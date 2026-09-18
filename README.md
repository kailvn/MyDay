[English](README.md) · [中文](README.zh-CN.md)

<div align="center">

# MyDay

**One window for your whole day. One file for your data. One CLI for everything else.**

![CI](https://github.com/kailvn/MyDay/actions/workflows/ci.yml/badge.svg)
![Version](https://img.shields.io/badge/version-1.0.0-blue)
![License](https://img.shields.io/badge/license-MIT-green)

[![Today](docs/screenshots/today-en.png)](docs/screenshots/today-en.png)

A planner for Linux and Windows that refuses to split your day across three
apps. Calendar, to-do, and journal share one local SQLite file — no account, no
sync, no cloud. Rust core, Tauri shell, and a command line that treats your
scripts and AI agents as first-class users.

**[Download](#get-started) · [CLI](#the-cli-is-the-product) · [中文文档](README.zh-CN.md)**

</div>

## The idea

A calendar knows what will happen. A to-do app knows what must get done. A
journal knows what actually happened. Keeping them in separate products keeps
your day in separate silos — and pushes you toward a cloud that holds the
pieces together. MyDay puts all three in **one file on your machine** and
builds everything on top of it:

- **Capture in seconds.** A global hotkey opens a bare input. Type a title,
  press Enter, done. What you filled in decides what it becomes — a due date
  makes a task, a time makes an event, plain text lands in the inbox. MyDay
  never makes you pick a type first.
- **Reminders you can trust.** A reminder is stored as intent ("10 minutes
  before the start"), not as a frozen timestamp. Move the meeting and the
  reminder follows. Sleep through a week of notifications and you get one
  tidy summary instead of a storm.
- **Deterministic by design.** In an era of apps that parse your sentences and
  hope for the best, MyDay takes the opposite bet: strict time formats, exact
  error codes, no silent rewriting — ever. When it doesn't understand, it says
  so.
- **Agent-native.** Every action in the interface is a `myday` command with a
  stable JSON envelope, idempotency keys and `--dry-run`. The CLI is not an
  afterthought; it is the second half of the product, and your automation runs
  on the same database as the window on your screen.

| | |
|---|---|
| [![Calendar](docs/screenshots/calendar-month-en.png)](docs/screenshots/calendar-month-en.png) | [![Quick add](docs/screenshots/quick-add-en.png)](docs/screenshots/quick-add-en.png) |
| [![Week](docs/screenshots/calendar-week-en.png)](docs/screenshots/calendar-week-en.png) | [![Stats](docs/screenshots/stats-en.png)](docs/screenshots/stats-en.png) |

Month grid with holiday badges, the capture window that infers the type, a
week time grid built for drag-and-drop, and a stats dashboard you can
rearrange like widgets.

## What it does

- **Calendar** in month / week / day: drag blocks to reschedule (15-minute
  snap), drag edges to resize, drag on empty space to create, drop tasks from
  the day panel onto any date. Recurrence with "edit the whole series"
  semantics — drag a recurring block to another weekday and the rule rewrites
  itself.
- **Tasks** in Today / Upcoming / All / Done, with batch actions and
  five-second undo; recurring tasks advance to the next occurrence when
  completed.
- **Logs** — pin a template (meds, weight, runs) for one-tap logging on the
  journal page; a composable stats dashboard turns the same data into heatmaps,
  streaks and trends with switchable chart types.
- **Today overlay** — a borderless, always-on-top mini window listing today's
  unfinished items. Lock it click-through, dial the opacity, park it in a
  corner.
- **Views** — Anytype-style filters and sorting on every list; the GUI and
  `myday item query` evaluate through the same engine.
- **Bilingual** — full English and Chinese interfaces, switchable in Settings;
  even the tray menu and system notifications follow.

## Get started

Rust 1.98+, Node 22 + pnpm; on Linux also
`libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev`.

```bash
cargo build                # CLI + core
cargo test                 # core / CLI / desktop tests
cd apps/desktop
pnpm install
pnpm tauri dev             # run the desktop app
pnpm tauri build           # deb installer
./scripts/build-windows.sh # Windows NSIS installer, cross-built from Linux
```

Your data lives in `~/.local/share/myday/` — one SQLite database plus an
attachment folder. Copy the directory to migrate. Back up with one command.
Schema upgrades only ever move forward.

## The CLI is the product

```bash
myday item add --title "Buy milk" --due 2026-09-16        # bare text → task
myday item add --title "1:1" --start "2026-09-16T14:30" --end "2026-09-16T15:30"
myday item add --title "Weekly report" --due 2026-09-23 --recurse @weekly:3
myday item complete <id>      # recurring tasks advance to the next occurrence
myday item query --view view_builtin_tasks_today --json   # same engine as the GUI
myday search "kickoff" --json
myday stats --days 30 --json
myday export ics --output ~/calendar.ics
myday backup                  # zip of db + attachments, keeps 7
echo '{"title":"Call mom","due_at":"2026-09-16T00:00:00Z"}' | myday item add --stdin --json
myday item add ... --dry-run --idempotency-key agent-1
```

Stable envelope `{"ok":true,"data":…}` / `{"ok":false,"error":{code,message}}`;
exit codes `0` ok · `1` error · `2` usage · `3` not found · `4` conflict.
The grammar is documented once and kept backward-compatible, because a CLI
that breaks its callers breaks the whole point.

## Principles

- **One file holds the truth.** Everything is in `items` — events, tasks,
  logs — with database-level constraints so even a rogue `sqlite3` session
  cannot corrupt the model.
- **Types are forever.** An item's type is fixed at creation (you may change
  it once, before saving). Certainty over flexibility.
- **Fields are Notion-style.** Custom fields live in a registry; renames are
  free, deletes are soft, history is never rewritten.
- **Upgrades never eat data.** Schema changes ship as additive migrations; if
  a migration is missing, MyDay refuses to open rather than rebuild.

More in [docs/](docs/): [architecture](docs/ARCHITECTURE.md) ·
[feature inventory](docs/FEATURE-INVENTORY.md) ·
[interaction spec](docs/INTERACTION.md) ·
[view/filter model](docs/FILTER-SPEC.md) ·
[overlay window](docs/OVERLAY-SPEC.md) · [e2e testing](docs/E2E.md).

## Roadmap

- Unscheduled-tasks backlog: drag undated tasks onto the calendar
- Single-occurrence exceptions for recurring items; ICS subscription
- macOS build

Release history in [CHANGELOG.md](CHANGELOG.md) · License [MIT](LICENSE)
