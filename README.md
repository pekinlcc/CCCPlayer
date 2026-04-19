# CCCPlayer

> **Claude Code & Codex Player — burn compute, not your time.**
>
> A macOS desktop app that hands a local folder to two AI coding agents and
> walks away until the work is done. · 让两个 AI coding agent 接管本地目录，
> 按一次 Play 之后就别管了。

![CCCPlayer idle screen](docs/screenshots/idle.jpeg)

---

- [English](#english) · [中文说明](#中文说明)
- **Download / 下载**：[`dist/`](https://github.com/pekinlcc/CCCPlayer/tree/claude/macos-claude-code-client-Fhm6A/dist)
- **Source / 源码**：[github.com/pekinlcc/CCCPlayer](https://github.com/pekinlcc/CCCPlayer)

---

## English

### What's new in v1.4.1 (2026-04-19)

- **Fixed a stagnation-detector false positive** that killed the
  v1.4.0 test session at round 6. The detector used to push one entry
  per `GoalCheckResult`, so a two-agent disagreement (Claude=0 /
  Codex=3 repeating) produced an `[0,3,0,3,…]` sequence that trips the
  "not strictly decreasing for 3 rounds" guard. Now the reducer waits
  for both agents per round and pushes a single `max(claude,codex)`
  entry.
- **Tightened the shelved-item contract.** Goal-check is strictly
  read-only — `shelved[]` must mirror PRD's `## Shelved
  disagreements` section verbatim, not be invented mid-phase.
  Refining has a mandatory grep check: if it marks anything
  `status: shelved`, PRD's `## Shelved disagreements` section must
  contain a matching entry by turn end.
- **New end-of-session report on the terminal screen** (DONE /
  STOPPED / ERRORED) showing duration, rounds, per-agent tokens, both
  agents' final goal checks side by side (done / missing / shelved /
  rationale), a plain-English reason (stagnation, rate-limit, etc.),
  a chronological highlights timeline, and links to the artifacts
  directory.

### What's new in v1.4.0 (2026-04-18)

Philosophical overhaul of the agent prompts. `GOAL.md` is now the only
immutable benchmark in the system; everything else (PRD milestones,
sub-goals, earlier design decisions, even accepted review findings) is
explicitly declared as revisable evidence that agents can — and
should — change when new information contradicts it.

- **`common.md`** declares *"Only GOAL.md is sacred — everything else
  is revisable evidence"*. Every phase must ask first: does this move
  us closer to GOAL.md? Guardrail: pivots require a concrete reason,
  not just "I want to try something else".
- **Planning** now decomposes `GOAL.md` into a `## Sub-goals` list
  that later phases trace against; followed by a mandatory
  self-adversarial check (does every sentence of GOAL have a coverage
  chain in PRD?). PRD gains a `## Changelog` for revisions.
- **Implementing** asks, before each turn, *is the next milestone
  still the shortest remaining path to GOAL?* If not, revise the PRD
  in-turn instead of mechanically ticking it off. Output includes a
  `goal_anchor` line.
- **Reviewing** restructures into a goal-first `## Goal coverage
  pass`: every sub-goal judged `delivered | partial | missing` before
  any code nitpicking. Blocking findings require a `goal_link` field
  pointing to the GOAL.md sentence they relate to. New severity
  `path_drift` for cases where code follows PRD correctly but PRD
  itself has drifted from GOAL.
- **Refining** requires a `goal_impact` line on every response
  (accept/reject/shelve). New status `stale` for findings that refer
  to superseded PRD items.

### v1.3 series recap

- **v1.3.2**: ad-hoc codesign + `install.command` one-click installer
  bundled in `dist/CCCPlayer-<ver>-arm64-install.zip` so third parties
  can install without hand-typing `xattr`.
- **v1.3.1**: Claude tokens were ~25× under-reported (parser was
  ignoring `cache_read_input_tokens`); fixed. Long goals now wrap in
  the Track line instead of being silently chopped at 60 chars.
- **v1.3.0**: agent disagreement protocol (accept / partial / reject /
  shelve); PRD becomes a living document; Codex rate-limit detected
  and routed to PAUSED with auto-resume scheduler; reviewing must
  write a new `codex_review_v{N+1}.md` or the turn is classified
  OutputMalformed.

### v1.2 recap

- GOAL_CHECK now runs after every REVIEWING (not only when Codex
  approves); Remaining panel refreshes every 2–10 minutes.
- Resume button after pause.
- Optimistic `⏸ PAUSING…` / `■ STOPPING…` title-bar feedback.

### v1.1 recap

- Remaining-to-goal panel with dual-column (Claude / Codex) + agreement
  badge showing the DONE contract (both agents done=true).
- Per-agent labeled Tokens KV (`Claude N` / `Codex N`).
- Native `Browse…` folder picker.
- Fixed: title-bar status / phase display was frozen at initial value
  due to a CamelCase regex mismatch; Pause looked broken as a result.
- Event schema: `GoalCheck` carries full `missing[]` + `rationale`.

Full changelog: [`RELEASE_NOTES.md`](RELEASE_NOTES.md).

### What is this

Set a local folder + describe a goal in plain English (e.g. *"pixel-perfect
clone of some.app, but drop the paid features"*). Hit the ▶ play button.
CCCPlayer drives **Claude Code** and **Codex** on your machine in a loop —
Claude plans + implements, Codex reviews, they iterate over a shared `PRD.md`
and `codex_review_v{N}.md` until both agree the goal is met.

The name is a nod to Winamp: this is a *player* for two code agents.
Triangle ▶ Play, double-bar ⏸ Pause, square ■ Stop. Neon cyan / magenta / lime
on black chrome. Everything runs locally; the app itself makes zero network
calls. Your tokens, your code, your machine.

### Prerequisites

macOS 13+ with both CLIs installed and logged in **inside your Terminal**:

```sh
# Claude Code CLI (Anthropic)
# Install per https://docs.anthropic.com/en/docs/claude-code
claude --version

# Codex CLI (OpenAI, via npm)
npm i -g @openai/codex
codex --version
codex login
```

Tokens are billed to your Claude + Codex accounts — CCCPlayer is just the
harness, not a proxy.

### Download

Latest Apple Silicon build lives in [`dist/`](dist/):

| File | Size | How to install |
| --- | --- | --- |
| [`CCCPlayer-<ver>-arm64-install.zip`](dist/) | ~4.5 MB | **Easiest.** Unzip, double-click `install.command`. First run: right-click → Open to clear the Gatekeeper script warning. |
| [`CCCPlayer-<ver>-arm64.dmg`](dist/) | ~5.7 MB | Traditional macOS install: mount, drag into `/Applications`. Then see "unblock" below. |
| [`CCCPlayer-<ver>-arm64.app.tar.gz`](dist/) | ~4.5 MB | Just the `.app` — for developers / CI. |

Apple Silicon only for now (M1/M2/M3/M4). Intel builds can be produced from
source (see *Build from source* below).

### Unsigned build — why and how to unblock

The build is ad-hoc signed but **not notarized** — there is no paid Apple
Developer Program account behind the release. On a fresh Mac, Gatekeeper
will refuse the first launch with "CCCPlayer cannot be opened because the
developer cannot be verified". Three ways to get past it:

1. **`-install.zip` + `install.command`** (recommended).
   The installer script clears the quarantine flag and copies the app to
   `/Applications`. The script itself may also trip Gatekeeper the first
   time — right-click it → Open → confirm once.
2. **Right-click → Open** on `CCCPlayer.app` directly, confirm in the
   dialog. Subsequent launches work normally.
3. **Strip quarantine in Terminal** (power user):
   ```sh
   xattr -cr /Applications/CCCPlayer.app
   open /Applications/CCCPlayer.app
   ```

### How to use

1. **Open CCCPlayer.app**. Top-right says `◇ NO SESSION` until you start one.
2. Fill **FOLDER** with a local path — empty dir or existing project, both
   work. CCCPlayer surveys existing code first and builds on top, never wipes.
   Home dir / system paths are rejected.
3. Type your goal in the terminal-style box. One or two short paragraphs.
4. Wait for both preflight rows to turn green (CLAUDE CODE CLI + CODEX CLI).
5. Click the lime **▶ Play** button in the bottom-left transport.
6. The UI switches to Running state:

    ![CCCPlayer running screen](docs/screenshots/running.jpeg)

   - Phase LEDs progress through `PLANNING → IMPLEMENTING → REVIEWING →
     REFINING → GOAL_CHECK`;
   - Progress panel shows live Elapsed · Tokens · Last activity · Round ·
     Failures (color-coded so you can glance from 20 feet away);
   - Left pane: structured Timeline (lime = ok, red = failed, amber = note);
   - Right pane: raw stdout/stderr from both CLIs, ring-buffered to 2000 lines.
7. **You can close the window and walk away.** Close ≠ quit — the session
   keeps running in the background. Use Cmd+Q to actually exit (it'll confirm).
8. When done: a result banner replaces the Progress panel with two buttons —
   **New session (same folder)** or **← Back to start**.

### What lives in your workdir

CCCPlayer stashes runtime state inside your workspace — auto-added to
`.gitignore` on session start:

![.cccplayer internals](docs/screenshots/progress.jpeg)

```
your-workdir/
├── GOAL.md               immutable — your goal, agents can't edit
├── PRD.md                the shared product spec Claude writes
├── codex_review_v1.md    Codex's review (one per round)
├── codex_review_v2.md
├── …
└── .cccplayer/           runtime state (gitignored)
    ├── session.json      state machine · round counter · usage totals
    ├── events.log        JSONL event stream, append-only
    ├── transcripts/      per-turn stdout + stderr
    └── snapshots/        round-00…round-N tar.zst snapshots
```

`round-00.tar.zst` is your original code — never deleted, so you can roll back.

### Build from source

```sh
# Rust-only checks (no macOS deps needed):
cargo check --workspace
cargo test  --workspace

# Frontend
cd ui && npm install && npm run build && cd ..

# macOS .app bundle (must be on macOS — no cross-compile path):
./scripts/build-macos.sh --arch arm64    # Apple Silicon
./scripts/build-macos.sh --arch x86_64   # Intel
./scripts/build-macos.sh                 # Universal
./scripts/build-macos.sh --dev           # Hot-reload dev mode
```

Artifacts land in `target/<triple>/release/bundle/macos/CCCPlayer.app`.

### Architecture at a glance

```
crates/
├── core/       cccplayer-core      state machine · persistence · snapshots
├── harness/    cccplayer-harness   CLI runner · stall watcher · orchestrator
└── fake-cli/   cccplayer-fake-*    used by integration tests (no token burn)
app/
└── src-tauri/  Tauri desktop shell + IPC commands + icon
ui/             React + Vite frontend (Shell / Welcome / Running / Terminal)
prompts/        default agent prompt templates (embedded at build)
scripts/        build-macos.sh
dist/           latest macOS artifacts
docs/           screenshots
```

Full design in [`PRD.md`](PRD.md). Line-by-line conformance in
[`AUDIT.md`](AUDIT.md). Approved visual mockup
[`ui/mockups/cyberpunk-preview.html`](ui/mockups/cyberpunk-preview.html).

### Key engineering choices

- **Single-writer state machine** — every transition flows through
  `core::reducer::Reducer` behind one Tokio mutex; side-effects never write
  state back.
- **Harness owns the CLI** — `HarnessRunner` spawns each agent in a fresh
  process group (`setsid` + `kill_on_drop`), redacts stdout/stderr inline,
  and watches for stalls on `CLOCK_MONOTONIC` (lid-close doesn't trip it).
- **Two Tauri event channels** — `cccplayer://event` carries structured
  events; `cccplayer://raw` streams annotated stdout/stderr lines so you can
  diagnose CLIs that crash before producing structured output.
- **Claude 2.x adaptation** — `-p` + stream-json requires `--verbose`; usage
  parsing handles nested `assistant.message.usage`; goal-check JSON extracted
  from stream-json envelopes by pulling `.message.content[*].text`
  and `.result`.
- **Codex 0.1x adaptation** — invoked via `codex exec <prompt>`; token count
  scraped from its plain-text `tokens used\n<N>` tail.
- **Finder-launched PATH fix** — the app prepends Homebrew / nvm / volta /
  bun / cargo bin dirs to `PATH` at startup so `#!/usr/bin/env node` scripts
  (Codex) can find `node` even without a Terminal-inherited environment.
- **Zero telemetry** — no network calls from the app itself.

### Status

M1 scope (two-agent loop + macOS shell + cyberpunk UI) is shipped and
packaged. M2 is open: interactive 3-way merge for PRD conflicts, TCC
deep-links, full prompt-override settings page, multi-window sessions. See
[`AUDIT.md`](AUDIT.md) for row-by-row status.

### Contributing / Issues

This is a personal project — open a PR or file an Issue on GitHub.

---

## 中文说明

### v1.4.1 新增（2026-04-19）

- **修复停滞检测误判**。v1.4.0 测试 session 在 round 6 被反停滞
  kill，根因是 `missing_history` 每次 `GoalCheckResult` 推一次——两 agent
  分歧（Claude=0 / Codex=3 反复）产出 `[0,3,0,3,…]` 序列，满足"连续 3
  轮未严格递减"触发停滞。改为双方都报完一轮后只推一次 `max(claude, codex)`。
- **收紧 shelved 合同**。goal-check 严格只读，`shelved[]` 必须照抄 PRD
  的 `## Shelved disagreements` 节，不能现场发明；refining 加强制
  grep check——标了 `status: shelved` 就必须在 PRD 里写对应条目，否则算
  本轮未完成。
- **结束界面新增执行报告**（DONE / STOPPED / ERRORED）。展示时长、轮数、
  两 agent 各自 token、双栏显示最终 goal check 分歧（done / missing /
  shelved / rationale），plain 英文 reason（stagnation / rate-limit 等），
  按时间轴的高亮事件列表，以及 artifacts 路径。

### v1.4.0 新增（2026-04-18）

Prompt 层哲学级改造。`GOAL.md` 是全系统**唯一不可变**的评估基准；
其他一切（PRD、milestone、子目标、过往设计决策、甚至已接受的 review
findings）都是可被修改的「证据假设」——当新证据推翻旧假设时，agent
可以、应该去改它。

- **`common.md`** 加入核心原则：*"Only GOAL.md is sacred — everything
  else is revisable evidence"*。每个 phase 都要先问：这个动作让我们
  离 GOAL.md 更近吗？Guardrail：必须给出具体证据才能 pivot，不允许
  「我想换个做法」这种空话（防止反复改主意不落实）。
- **Planning** 加 Step 1 目标拆解（`GOAL.md` 拆成可独立判 done 的
  `## Sub-goals` 清单）+ Step 3 保存前自检（逐句对照 GOAL 确认每句都
  有覆盖链 sub-goal → scope → design → milestone）。PRD 新增
  `## Changelog` 节记录所有后续修订。
- **Implementing** 开头强制自问：下一个 milestone 依然是到 GOAL 的最短
  路径吗？不是就先改 PRD（标记 superseded + Changelog 一行）再动代码。
  stdout 输出 `goal_anchor` 和 `plan_revised` 字段。
- **Reviewing** 重构为 goal-first：新增 `## Goal coverage pass` 节先
  逐个 sub-goal 判 delivered / partial / missing，才进入代码级 findings。
  每条 blocking 必填 `goal_link`（指回 GOAL.md 的哪句）。新 severity
  `path_drift`：代码对但 PRD 偏了，修法是改 PRD 不是改代码。
  `status: approved` 要求 blocking=[] 且 path_drift=[]。
- **Refining** 要求每条响应都带 `goal_impact` 一句话（accept 的话说
  为什么让 goal 更近；reject/shelve 的话说为什么不做也不伤 goal）。
  新 status `stale` 用于已被 PRD Changelog superseded 的 finding。

### v1.3 系列回顾

- **v1.3.2**：ad-hoc 签名 + `install.command` 一键安装脚本，打包到
  `dist/CCCPlayer-<ver>-arm64-install.zip`，第三方装机不用手工 `xattr`。
- **v1.3.1**：Claude token 被低估 25× 修复；Track 长目标折行不截断。
- **v1.3.0**：双 agent 分歧协议（accept / partial / reject / shelve）；
  PRD 成为活文档；Codex 配额耗尽 → PAUSED + 自动恢复定时器；REVIEWING
  没产生新 review 文件直接降级 OutputMalformed。

### v1.2 回顾

- 每次 REVIEWING 完都跑 GOAL_CHECK（不再等 Codex Approved），Remaining 面板
  2-10 分钟刷一次。
- Pause 之后出现 Resume 按钮。
- Pause/Stop 乐观反馈——点下去立刻 `⏸ PAUSING…` / `■ STOPPING…`。

### v1.1 回顾

- Remaining-to-goal 面板双栏（Claude / Codex）+ agreement 徽章，把「DONE
  需要双方一致」的契约可视化。
- Tokens 按 agent 分行标注（`Claude N` / `Codex N`）。
- 原生 `Browse…` 目录选择器。
- 修复：state_changed CamelCase 正则 bug 导致 UI 状态永远不刷，Pause 看起来
  没反应其实是 session 早已 Errored。
- 事件 schema：`GoalCheck` 带全量 `missing[]` + `rationale`。

完整变更见 [`RELEASE_NOTES.md`](RELEASE_NOTES.md)。

### 这是什么

**CCCPlayer（Claude Code & Codex Player）**。叫「Player」是想到当年的 Winamp。
设定一个本地文件夹和你要达成的目标，比如「完全复刻某某 app 的功能，但去掉收费
模块」，你电脑上的 **Claude Code** 和 **Codex** 就会轮流干活并互相检查——
Claude 规划 + 实现，Codex 评审，两者通过共享的 `PRD.md` 和 `codex_review_v{N}.md`
反复迭代，直到双方都认为目标已达成。

一次 Play 按下去，然后就可以合盖去干别的。产品哲学是：**烧算力，不烧你的时间。**

### 使用前提

在你的 **macOS Terminal 内同时配置并登录** Claude Code 和 Codex CLI：

```sh
# Claude Code CLI（Anthropic 官方）
# 按 https://docs.anthropic.com/en/docs/claude-code 的指引安装
claude --version

# Codex CLI（OpenAI 官方，npm 安装）
npm i -g @openai/codex
codex --version
codex login
```

整个过程中 **完全使用你本人的 Claude Code 和 Codex 账户 + token**。CCCPlayer
本身不发任何网络请求，不是代理。

### 下载

最新 Apple Silicon 构建产物在 [`dist/`](dist/)：

| 文件 | 大小 | 安装方式 |
| --- | --- | --- |
| [`CCCPlayer-<ver>-arm64-install.zip`](dist/) | ~4.5 MB | **推荐**。解压后双击 `install.command`，第一次右键→打开一次确认，脚本自动去掉 quarantine + 复制到 `/Applications` + 启动 |
| [`CCCPlayer-<ver>-arm64.dmg`](dist/) | ~5.7 MB | 传统 macOS 装法：挂载 DMG → 拖进 `/Applications`。之后按下方"解除 Gatekeeper"操作 |
| [`CCCPlayer-<ver>-arm64.app.tar.gz`](dist/) | ~4.5 MB | 纯 `.app` —— 给开发者 / CI 用 |

目前只有 Apple Silicon（M1/M2/M3/M4）版本。Intel 可以自己编（见下方「从源码编译」）。

### 未签名的说明 · 三种解除 Gatekeeper 的方法

本次构建做了 **ad-hoc 签名** 但**没有公证**——因为没买 Apple Developer
Program（$99/年）。别人首次下载打开会被 macOS 拦「无法打开，因为无法验证
开发者」。三选一解除：

1. **用 `-install.zip` + `install.command`**（最省事）。
   脚本自动清 quarantine + 拷到 `/Applications` + 启动。脚本本身首次也会
   被 Gatekeeper 拦，**右键脚本 → 打开 → 确认**即可，仅一次。
2. **右键 CCCPlayer.app → 打开**，对话框里再点一次"打开"。后续双击正常。
3. **终端放行**（懂命令行的）：
   ```sh
   xattr -cr /Applications/CCCPlayer.app
   open /Applications/CCCPlayer.app
   ```

### 怎么用

1. **双击打开 CCCPlayer.app**。初始化界面如下，右上角显示 `◇ NO SESSION`：

    ![初始化界面](docs/screenshots/idle.jpeg)

2. **FOLDER** 填工作目录（随便挑一个本地项目路径，已有代码或空目录都行——
   CCCPlayer 会先盘点现状再在上面增量，不会清空）。禁止家目录 `~` 或系统路径。
3. **Goal** 里用自然语言写你想让它做的事，一两段话足够：
    > 例：「帮我像素级复刻 vibeisland.app，但不要做收费模块，其他都保持一致。」
    > 例：「给这个项目加 CSV 导入并补完集成测试。」

4. 等底下两行 CLI 状态都变绿（CLAUDE CODE CLI ● + CODEX CLI ●）。
5. 点左下角绿色 **▶ Play**。客户端就调用你电脑上的 Claude Code 和 Codex 开始干活，
   过程中先由 Claude Code 规划和实现，Codex 来检查：

    ![运行中界面](docs/screenshots/running.jpeg)

   - 上方 Phase LED 灯条按 `PLANNING → IMPLEMENTING → REVIEWING → REFINING →
     GOAL_CHECK` 推进；
   - Progress 面板实时显示 Elapsed（已耗时）/ Tokens（累计 token）/ Last activity
     （上次输出距今多久，超 60 秒会变黄警告可能卡住）/ Round（第几轮）/ Failures
     （失败计数）；
   - 左栏 Timeline 是结构化事件（ok 绿、失败红、note 琥珀）；
   - 右栏 Raw stream 是两个 CLI 的原始 stdout/stderr，环形缓冲 2000 行。

6. **可以关窗走人**。关窗 ≠ 退出——进程继续在后台跑。真要退出请 Cmd+Q（会弹确认）。
7. 结束时（DONE / STOPPED / ERRORED）Progress 面板会替换为结果横幅，两个按钮：
   **New session (same folder)** 保留目录重开、**← Back to start** 清空重来。

### 过程中的产物

Claude Code 和 Codex 的交流过程保存在工作目录下的隐藏文件夹 `.cccplayer/` 中
（session 创建时会自动加到 `.gitignore`）：

![.cccplayer 文件夹内容](docs/screenshots/progress.jpeg)

```
your-workdir/
├── GOAL.md               你的目标（被锁，agent 不许改）
├── PRD.md                Claude 写的产品需求文档（两 agent 的共识）
├── codex_review_v1.md    Codex 的评审（每 round 一个版本）
├── codex_review_v2.md
├── …
└── .cccplayer/           运行时私有（gitignored）
    ├── session.json      状态机当前态 / round 计数 / 累计 usage
    ├── events.log        结构化事件流 JSONL，append-only
    ├── transcripts/      每个 turn 的 stdout + stderr 归档
    └── snapshots/        round-00…round-N 的 tar.zst 快照，可回滚
```

`round-00.tar.zst` 是你**原始代码的完整备份**——永远不会被删，出错能回到起点。

### 从源码编译

```sh
# 纯 Rust 检查（不需要 macOS 原生依赖）
cargo check --workspace
cargo test  --workspace

# 前端打包
cd ui && npm install && npm run build && cd ..

# macOS .app 打包（必须在 Mac 上跑，没有 Linux→macOS 交叉编译路径）
./scripts/build-macos.sh --arch arm64    # Apple Silicon
./scripts/build-macos.sh --arch x86_64   # Intel
./scripts/build-macos.sh                 # Universal
./scripts/build-macos.sh --dev           # 开发热重载模式
```

产物在 `target/<triple>/release/bundle/macos/CCCPlayer.app`。

### 架构速览

```
crates/
├── core/        cccplayer-core      状态机 / 持久化 / 快照 / 脱敏
├── harness/     cccplayer-harness   CLI runner / stall watcher / orchestrator
└── fake-cli/    cccplayer-fake-*    集成测试用假 CLI（不烧 token）
app/
└── src-tauri/   Tauri desktop 壳 + IPC 命令 + 图标
ui/              React + Vite 前端（Shell / Welcome / Running / Terminal）
prompts/         默认 prompt 模板（编译期嵌入）
scripts/         build-macos.sh
dist/            最新 macOS 产物
docs/            截图等
```

详细设计见 [`PRD.md`](PRD.md)，实现细节的行级对照见 [`AUDIT.md`](AUDIT.md)。
UI 视觉原型图：[`ui/mockups/cyberpunk-preview.html`](ui/mockups/cyberpunk-preview.html)
（浏览器直接打开）。

### 关键工程决策（节选，完整版见 PRD §16 / §18）

- **单写者状态机**——所有状态跳转都经过 `core::reducer::Reducer`，一个 Tokio
  mutex 保护，side-effect 不回写 state。
- **harness 独占 CLI**——`HarnessRunner` 用 `setsid` + `kill_on_drop(true)` 在
  独立进程组里跑 agent，stdout/stderr 实时脱敏，stall watcher 走
  `CLOCK_MONOTONIC`（合盖不会误判卡死）。
- **Tauri 双事件通道**——`cccplayer://event` 结构化事件 + `cccplayer://raw`
  带 agent/phase 标注的原始流，UI 2000 行环形缓冲。
- **Claude 2.x 适配**——`-p` + stream-json 必须加 `--verbose`；usage 解析兼容
  嵌套 `assistant.message.usage`；goal-check JSON 从 stream-json 外壳里抽
  `.message.content[*].text` 与 `.result`。
- **Codex 0.1x 适配**——走 `codex exec <prompt>`；token 计数扫
  `tokens used\n<N>` 明文。
- **Finder 启动 PATH 注入**——app 启动时把 Homebrew / nvm / volta / bun / cargo
  的 bin 目录前插到 `PATH`，保证 `#!/usr/bin/env node` 的 CLI（Codex）也能找到
  `node`。
- **零遥测**——app 本身不发任何网络请求。

### 当前状态

M1 范围（双 agent 循环 + macOS 壳 + 赛博朋克 UI）已落地并打包。M2 未完成：
交互式三路合并、TCC 深链、完整 prompt 覆盖设置页、多窗口 session。逐条对照
见 [`AUDIT.md`](AUDIT.md)。

### 反馈 / 提 Issue

这是个人项目，欢迎直接开 PR 或在 GitHub 仓库里开 Issue。
