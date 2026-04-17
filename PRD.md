# CCCPlayer PRD

> Claude Code × Codex 双 Agent 循环执行器（macOS 客户端）
> 版本：v0.1（初稿）
> 日期：2026-04-17

---

## 1. 产品定位

CCCPlayer 是一款 macOS 本地客户端。用户只需要用自然语言描述一个"目标"，点击"开始"，
应用就会在本地 Terminal 中交替驱动 **Claude Code** 与 **Codex** 两个 agent harness：
Claude Code 负责设计与实现，Codex 负责评审，两者通过一组共享文档
（`PRD.md`、`codex_review_v{n}.md`）反复来回，直到双方一致认为目标已达成。

核心价值：
- 把"自我对弈式"的多 agent 协作收纳成一个**一键启动、可中断可恢复**的工作流。
- 用户不需要写 prompt、不需要手动粘贴评审结论，全部由客户端调度。
- 所有中间产物是**人类可读的 Markdown**，随时可以暂停、审阅、甚至手工修改后继续。

设计哲学：**烧算力，不烧用户时间**。应用默认不对 token 或 wall-clock 设任何上限；
只在真正卡死（进程无心跳）或真正震荡（多轮无进展）时才中断。用户按"开始"之后
希望回来看到的是"已完成"，而不是"因为跑得太久所以停了等你确认"。

## 2. 目标用户 & 前置条件

- 目标用户：在 macOS 上已配置好 Claude Code 与 Codex CLI 的开发者。
- 前置条件（应用启动时检查）：
  1. 系统：macOS 13+（Apple Silicon / Intel 皆可）。
  2. `claude` CLI 已安装、已登录（能以非交互方式执行）。
  3. `codex` CLI 已安装、已登录。
  4. 用户已选定一个**工作目录**（project workspace），应用拥有读写权限。
  5. `git` 可用（用于快照与回滚，详见 §8）。

不满足时，应用以"前置检查"页面引导用户逐项解决，而非在运行中失败。

### 2.1 工作目录的四种起始形态

CCCPlayer 必须优雅处理工作目录的所有合理起始状态。**不假设空目录**。

| 起始形态 | 处理 |
| --- | --- |
| a. 空目录 | 新建 Session：从零开始 PLANNING → IMPLEMENTING。 |
| b. 已有用户代码，无 CCCPlayer 痕迹 | 新建 Session：PLANNING 的第一步是**现状盘点**（见 §10、§17.1）——先读一遍工作树，再写 PRD.md 的 "Current state" 节，PLANNING 之后的实现把现有代码当作**已完成的增量**。 |
| c. 已有本应用 Session（`.cccplayer/` 存在）且 `GOAL.md` 与用户这次输入一致 | 直接恢复旧 Session（见 §7 决策树）。 |
| d. 已有本应用 Session 但 `GOAL.md` 与这次输入不一致 | 弹窗询问：**继续旧目标** / **基于现状换新目标（归档旧 Session）** / **取消**。不静默覆盖。 |

形态 b 是实战中最常见的情形——用户可能在别的工具里写了一半、或是一个真实的现有
项目。CCCPlayer 的 PLANNING 阶段必须把"现有代码"当成不可忽视的约束与资产，而不
是推倒重来。

## 3. 用户故事

- **US-1 一键启动**：我输入"做一个能在本地跑的 Markdown TODO CLI，用 Rust 写"，
  点"开始"，回来吃个饭，期望看到一个可运行的雏形 + 完整 PRD + 若干轮 Codex 评审。
- **US-2 意外中断**：跑到一半我锁屏 / 关机 / Kill 掉 Terminal。回来再点"开始"，
  应用应判断"目标是否已完成"，没完成则从最近一次稳定状态继续，不重头跑。
- **US-3 手动干预**：我看到某一版 `codex_review_v3.md` 指出一个安全问题，我想自己
  改两行代码再继续。暂停 → 编辑 → 继续，循环从当前工作树重新进入评审。
- **US-4 终止**：我发现目标定偏了，点"停止"。再次"开始"时，应用询问是继续旧目标、
  开新会话，还是基于旧工作树换新目标。
- **US-5 基于现有代码**：我把一个已经写了一半的 Rust 项目的目录选给 CCCPlayer，
  输入目标"让它支持从 CSV 导入并加集成测试"，点开始。CCCPlayer 应先盘点现状（不
  重写已有部分），把 PRD.md 写成"从当前状态到目标"的增量计划，然后在现有代码上
  迭代，而不是生成一个和现有目录无关的全新项目。

## 4. 核心概念与术语

| 术语 | 含义 |
| --- | --- |
| Goal | 用户输入的自然语言目标。**写入工作目录下的 `GOAL.md`**，Claude Code 与 Codex 都直接读它，不再通过 prompt 注入。Session 进行中不可编辑；要改目标需新建 Session。 |
| Session | 一次"目标→完成"的端到端过程，对应一个工作目录 + 一份持久化状态。**应用同一时刻仅支持一个活动 Session**，避免对同一工作目录的并发写入与 CLI 配额争抢。 |
| Round | 一轮"实现→评审"循环，递增整数 `n`，对应一份 `codex_review_v{n}.md`。 |
| Turn | Round 内的一次 agent 调用（实现 turn 或评审 turn）。 |
| Verdict | Codex 在评审末尾给出的结构化判定：`approved` / `changes_requested` / `blocked`。 |
| Harness | 对某个 CLI（`claude` / `codex`）的封装，负责进程管理、日志、超时、终止信号。 |

## 5. 工作流（状态机）

```
        ┌──────────────┐
        │   CREATED    │  新建 Session，已保存 Goal
        └──────┬───────┘
               │ start
               ▼
        ┌──────────────┐
        │   PLANNING   │  Claude Code 产出/更新 PRD.md
        └──────┬───────┘
               │ PRD 就绪
               ▼
        ┌──────────────┐
        │ IMPLEMENTING │  Claude Code 写/改代码
        └──────┬───────┘
               │ 实现完成一轮
               ▼
        ┌──────────────┐
        │  REVIEWING   │  Codex 生成 codex_review_v{n}.md
        └──────┬───────┘
               │
               ▼
     ┌────────────────────┐
     │  verdict ==        │
     │   approved?        │─── yes ──► DONE
     └─────────┬──────────┘
               │ no
               ▼
        ┌──────────────┐
        │  REFINING    │  Claude Code 阅读评审、附上回应、改代码
        └──────┬───────┘
               │ n+1 轮
               └────► REVIEWING
```

状态机还包含横切状态：

- `PAUSED`：任何状态下用户点"暂停"或检测到宿主挂起都会进入，保留上次在途 turn 的输入/日志。
- `ERRORED`：harness 连续失败达阈值时进入，等待用户介入。
- `ABANDONED`：用户显式"停止并放弃"后的终态。

**GOAL-CHECK 触发点**（明确，避免歧义）：
1. 用户点"开始"且 Session 不在 RUNNING 类状态时，**串行**跑一次（先 Claude Code，
   再 Codex；任一返回 done=false 就立刻按其结论进入下一态，不浪费另一次调用）。
2. 每个 REVIEWING turn 完成后，若 Codex verdict == `approved`，**并行**跑两个
   GOAL-CHECK；两个都 done=true 才进 DONE，否则按 §7 的分歧规则继续。
3. 其他时刻不跑 GOAL-CHECK（避免无谓调用）。

## 6. UI / 交互设计

一个极简单页应用，参考"唱片机"的隐喻（呼应 CCCPlayer 名字）：

```
┌──────────────────────────────────────────────┐
│  CCCPlayer                               ⚙︎   │
├──────────────────────────────────────────────┤
│  目标                                         │
│  ┌────────────────────────────────────────┐  │
│  │ 多行输入框（支持 Markdown 粘贴）          │  │
│  └────────────────────────────────────────┘  │
│                                              │
│  工作目录： ~/dev/todo-cli         [选择…]    │
│                                              │
│       ┌─────────┐  ┌─────────┐               │
│       │  开始   │  │  暂停   │   Round 3/∞   │
│       └─────────┘  └─────────┘               │
│                                              │
│  当前阶段： REVIEWING (Codex) ████░░░░░░     │
│  最近产出： codex_review_v3.md  [打开]        │
│                                              │
│  ── 事件流 ─────────────────────────────────  │
│  13:04  Claude Code 已更新 PRD.md (+42/-8)    │
│  13:11  Claude Code 实现完成，耗时 6m41s      │
│  13:12  Codex 开始评审…                      │
│  13:18  Codex verdict: changes_requested (4) │
│  …                                           │
└──────────────────────────────────────────────┘
```

关键交互点：
- **"开始"是幂等的**：无论当前 Session 是 CREATED / PAUSED / ERRORED / DONE，
  点下去都会先做一次"目标是否已达成"的检测再决定动作（见 §7）。
- 设置页：模型选择、无心跳超时（默认 10 分钟）、可选 Round 数硬帽（默认关闭）、
  自定义 prompt 模板、CLI 绝对路径。

### 6.1 进度显示：三段式

为了避免"长 turn 里 UI 看起来卡死"和"直接裸露 stdout 太吵"两个极端，运行中的
主界面分为三个区域：

1. **顶部状态条**：当前阶段（PLANNING/IMPLEMENTING/REVIEWING/REFINING/…）、
   Round N、已耗时、最近一次心跳（从 stream-json 中拿到最新事件的时间戳）。
2. **事件时间线（默认主视图）**：结构化事件流，不是 stdout 原文。数据源是两个 CLI
   的 stream-json 输出（`claude --output-format stream-json`、Codex 对应模式），
   我们解析后产出如下事件类型：
   - `AgentStarted {agent, phase}`
   - `FileEdited {path, added, removed}` / `FileCreated {path}` / `FileRead {path}`
   - `ToolInvoked {name, summary}`（如 `cargo check`、`rg "TODO"`）
   - `ReviewWritten {version, verdict, blocking_count}`
   - `GoalCheck {agent, done, missing}`
   - `Error {kind, message}`、`Heartbeat {token_usage}`
   - `AgentFinished {agent, phase, duration}`
   
   每条事件可点开看对应的 transcript 片段。
3. **原始日志抽屉**（右侧可折叠）：实时流 stdout+stderr，带搜索；默认收起，给
   debug 使用，不构成日常主视图的一部分。

事件时间线与状态条是"产品态"，原始日志抽屉是"开发者态"——两层分离，保证默认
视图不像 Terminal、需要时又能一键下钻到 Terminal。

### 6.2 首次启动 / 空白态

应用第一次启动、或当前没有任何活动 Session 时，主区域显示一个简短欢迎页：

```
┌──────────────────────────────────────────────┐
│  CCCPlayer                                   │
│                                              │
│  让 Claude Code 和 Codex 替你来回打磨代码      │
│  直到目标达成。                                │
│                                              │
│  [  选择工作目录  ]                            │
│                                              │
│  Preflight                                   │
│  ● Claude Code CLI    检查中…                │
│  ● Codex CLI          检查中…                │
│  ● Claude 登录态      待目录选定后再检测        │
│  ● Codex 登录态       待目录选定后再检测        │
└──────────────────────────────────────────────┘
```

选定目录后：
- 若目录里已有 `.cccplayer/session.json` → 进入"恢复已有 Session"视图（顶部显示
  旧目标 + 旧状态 + 一键继续 / 归档新建）。
- 否则 → 显示目标输入框，preflight 跑完且全绿才启用"开始"。

### 6.3 工作目录的切换

活跃 Session 中（任何 RUNNING / PAUSED 中态）禁止切换工作目录——切换按钮灰显并
浮提示"先停止当前 Session 再切换"。这避免编辑器一样的"打开新文件丢失改动"陷阱。
Session 处于 DONE / ABANDONED / 未开始时可自由切换。

## 7. "开始"按钮的决策树

这是产品的核心鲁棒性来源，必须在一处集中实现：

```
on_click_start(session):
  ensure_preflight_ok()                       # CLI/登录/磁盘
  if session.state in {RUNNING, REVIEWING, IMPLEMENTING, REFINING, PLANNING}:
      return focus_event_stream()             # 已在跑，无事发生
  snapshot = load_last_stable_snapshot()
  verdict  = run_goal_check(snapshot)         # Claude Code 只读评估
  if verdict == DONE:
      show("目标已达成，无需继续"); return
  if verdict == NEEDS_PLANNING:
      transition(PLANNING)
  else:
      transition(REFINING or IMPLEMENTING)    # 根据缺失环节
  resume_loop()
```

"目标是否已达成"的判断走**两票一致制**：Claude Code 与 Codex 各做一次只读评估，
**两边都返回 `done=true` 才算达成**，任一方说没完成就继续。输入都是
`GOAL.md + PRD.md + 最新 codex_review_v{n}.md + 工作树文件清单/diff 摘要`，
输出结构化 JSON：

```json
{
  "done": false,
  "missing": ["未实现 CLI 的 --export 参数", "无集成测试"],
  "next_state": "IMPLEMENTING"
}
```

两边判定分歧时的处理：
- Claude 说 done、Codex 说未完成 → 按 Codex 的 `missing` 进 `REFINING`。
- Claude 说未完成、Codex 说 done → 按 Claude 的 `missing` 进 `REFINING`。
- 两边都判 done → 进 `DONE`。
- 若分歧在**同一 Round**内连续出现 3 次且 missing 列表稳定不变 → 判定"震荡"，
  进入 `ERRORED`，让用户介入（见 §11）。

## 8. 持久化与恢复

每个 Session 在工作目录下维护 `.cccplayer/`：

```
.cccplayer/
├── session.json          # 状态机、Round 计数、时间戳、schema_version
├── session.lock          # 文件锁，单 Session 互斥（见下）
├── events.log            # 追加式事件流（事件流 UI 的数据源）
├── usage.json            # 本 Session 累计 token / 成本（见 §16.7）
├── transcripts/
│   ├── round-01-impl.txt
│   ├── round-01-review.txt
│   └── …
└── snapshots/
    ├── round-00.tar.zst  # Session 刚创建时的"原始状态"快照，永不删除
    ├── round-01.tar.zst  # 每 Round 开始前对工作树打包（排除 .cccplayer 自身）
    └── …
```

`session.json` 必须含 `"schema_version": 1` 字段。后续 PRD 演进改字段时按 version
做迁移，不在此 PRD 展开迁移脚本。

`session.lock` 是 `flock(2)` 排他锁文件：
- 应用启动并打算操作某个工作目录前，先尝试持锁。失败 → 弹窗 "另一个 CCCPlayer
  实例已经在使用这个目录"，禁止打开。
- 持锁文件里写入当前进程 pid + 进程启动时间（`start_boottime`）；下次任何实例
  启动时若发现锁存在但匹配的 pid+start_time 已不存在，视作残留锁可清理。
- 仅匹配 pid 不够——pid 可被复用；必须 pid+start_time 双匹配，避免误杀新进程。

**`round-00` 快照是用户原始代码的保险丝**：无论后续 agent 改了多少，用户随时可以
"回到我最初给你的那份代码"，零风险地试。

**不动用户的 git**：工作目录被视为一个普通本地文件夹。
- 如果用户的目录恰好是 git 仓库，CCCPlayer 不调用 `git commit` / `git stash`，
  也不创建分支，避免污染用户的历史。
- 快照一律用 `tar + zstd` 落在 `.cccplayer/snapshots/` 下；回滚就是解压覆盖。
- **`.gitignore` 自动追加**：Session 创建时若工作目录根存在 `.git/`，自动在
  `.gitignore` 末尾追加 `.cccplayer/`（已存在则跳过）。事件流里通知用户做了这
  一步。这避免 `git add .` 把 GB 级 tar 包误提交。

恢复策略：
- 应用启动时扫描选中目录下的 `.cccplayer/session.json` 还原状态机。
- 若上次 turn 中断（进程被 kill），对应子进程 pid 记录已失效，回到该 turn 起点重跑。
- **不假设外部进程可靠**：所有 harness 调用都必须是"可重复执行、幂等或显式回滚"的。

## 9. Agent Harness 设计

两个 harness 共享一个接口：

```ts
interface Harness {
  kind: "claude" | "codex";
  run(input: TurnInput): AsyncIterable<TurnEvent>;   // 流式事件
  cancel(): Promise<void>;                           // SIGINT → SIGTERM → SIGKILL
}
```

- 通过 `spawn` 启动 CLI（非 PTY，必要时加 `script` 包装以获取彩色输出）。
- 入参通过 **stdin + 文件**两种方式：prompt 写入临时文件避免 shell 转义。
- 超时：单 turn 默认 20 分钟，可设置；超时走 cancel 协议。
- 日志：原样落盘到 `transcripts/`，UI 事件流只展示结构化摘要。
- 不做"prompt 工程魔法"：所有 system prompt 模板放在 `prompts/*.md`，用户可编辑。

### 9.1 Claude Code 的职责

| 阶段 | 输入 | 输出 |
| --- | --- | --- |
| PLANNING | `GOAL.md` | 新建或更新 `PRD.md` |
| IMPLEMENTING | `GOAL.md`、`PRD.md` | 代码改动（由 Claude Code 直接写盘） |
| REFINING | `codex_review_v{n}.md` | 在同一文件尾部追加 `## Claude Code 回应` 段，并修改代码 |
| GOAL-CHECK（只读） | `GOAL.md`、PRD、最新评审、diff 摘要 | 结构化 JSON |

### 9.2 Codex 的职责

| 阶段 | 输入 | 输出 |
| --- | --- | --- |
| REVIEWING | `GOAL.md`、`PRD.md`、工作树 | 新建 `codex_review_v{n+1}.md`，末尾必须含 verdict 段 |
| GOAL-CHECK（只读） | `GOAL.md`、PRD、最新评审、diff 摘要 | 结构化 JSON，与 Claude Code 的输出格式一致 |

### 9.3 GOAL.md 的约定

- 位置：工作目录根下的 `GOAL.md`。
- 内容：用户原样输入的自然语言目标，外加应用自动追加的只读头部（创建时间、
  Session ID）。
- 生命周期：创建 Session 时写入，之后对所有 agent **只读**。用户想改目标，应用
  引导新建 Session。
- 两个 agent 的 prompt 模板都以"第一步请读 `GOAL.md`"开头，保证一致理解。

Verdict 段格式（由 prompt 约束，解析器宽容）：

```markdown
## Verdict
- status: changes_requested
- blocking:
  - 未处理 CSV 解析中的 UTF-8 BOM
- non_blocking:
  - 可以考虑引入 clap 的 derive 特性
```

## 10. 文档规范

- `GOAL.md`：由应用在创建 Session 时写入，之后所有 agent 只读（见 §9.3）。
- `PRD.md`：由 Claude Code 全权维护。结构要求（由 prompt 约束）：
  `Goal → Current state → Scope → Non-goals → Design → Milestones → Open Questions`。
  - **Current state 节**：PLANNING 阶段 Claude Code 先扫描工作目录，产出对现有
    代码的盘点（技术栈、入口、关键模块、已实现功能、明显缺口）。若目录是空的，
    写 "empty workspace" 一行即可。Milestones 必须以这份盘点为起点定义增量。
- `codex_review_v{n}.md`：
  - 由 Codex 创建，单调递增，**不允许删除旧版本**。
  - Claude Code 在 `REFINING` 阶段追加 `## Claude Code 回应` 段，说明每条
    blocking 项的处理（接受 / 部分接受 / 拒绝+理由）。
  - 文件末尾必须有 `## Verdict` 段。

## 11. 停止条件

Round 循环**不设**整体时长上限，也不设 token 预算。只在以下情形停止：

1. **目标达成**（终态 `DONE`）：最近一轮 Codex verdict == `approved` **且**
   Claude Code 与 Codex 的 Goal-Check **都**返回 `done == true`。
2. **用户显式暂停 / 停止**。
3. **进程卡死**（进入 `ERRORED`）：stream-json 事件流连续 10 分钟无任何新事件
   （可在设置里调），视作 harness 无心跳，触发一次自动 cancel + 重试；再次卡死
   则停下来等用户介入。**注意**：这里的"10 分钟"必须用对睡眠免疫的时钟——
   `CLOCK_MONOTONIC` + `NSWorkspace` 唤醒通知重置基准（见 §16.4）。
4. **震荡 / 停滞**（进入 `ERRORED`，等用户介入），任一触发：
   - 连续 2 轮 Codex 评审的 blocking 列表高度相似（Haiku 做相似度判断）；
   - 同一 Round 内双方 Goal-Check 分歧 3 次且 `missing` 列表稳定不变；
   - **进展停滞**：连续 3 轮双方 Goal-Check 的合并 `missing` 列表大小**没有
     严格下降**（哪怕内容变了），视作"在原地换姿势"。
5. **认证失效**（进入 `PAUSED`）：harness 检测到 CLI 返回认证错误，提示用户
   登录后恢复（见 §16.2）。
6. **Agent 拒绝**（进入 `ERRORED`）：harness 识别到 CLI 输出疑似拒绝模式
   （exit 0 + 无文件改动 + 关键词如 "I can't"、"I won't"、"refuse" 等），
   **不重试**——直接提示用户调整目标。识别策略见 §16.8。

**默认没有** Round 数上限、没有 Session wall-clock、没有 token 预算。设置里提供
可选的 Round 数硬帽（默认关闭）给想兜底的用户。

## 12. 安全与权限

- 仅在用户选定的工作目录内读写；应用自身目录与系统目录访问走 macOS 的 TCC。
- 不代替用户登录 CLI；不持有 API key。
- 运行期默认**离线文档可见**、**联网由 CLI 本身决定**；应用不额外外联。
- **脱敏**：harness 在写 `transcripts/*.txt`、`events.log` **以及** UI 推流前都跑
  一遍正则脱敏（`sk-…`、`ghp_…`、`xoxb-…`、`AKIA…`、`-----BEGIN ...PRIVATE KEY-----`
  等常见模式）。脱敏在写盘前完成——一旦原文落盘就无法再保证。
- **零遥测**：CCCPlayer 自身不发送任何分析、日志或崩溃报告到外部。所有数据留
  在本地（工作目录 `.cccplayer/` 与 `~/Library/Application Support/CCCPlayer/`）。
  这是产品级承诺，不在设置里提供"启用遥测"开关。
- **TCC 拒绝处理**：用户首次选择目录时，若 macOS 抛出权限错误（EACCES），引导
  用户到 系统设置 → 隐私与安全性 → 文件与文件夹 中授予权限，并提供"重试"按钮。
- **自动批准 agent 工具调用**（关键）：见 §16.10。CCCPlayer 是无人值守编排器，
  必须让 agent 在工作目录范围内自由执行工具调用，否则循环会卡在第一次危险操作
  上等用户确认。这是一项有意识的权限取舍，UI 必须明示。

## 13. 里程碑

- **M1 MVP（2 周）**：单 Session、顺序 Round、UI 三要素（输入 / 开始 / 事件流）、
  Claude Code + Codex 打通、PRD.md 与 codex_review_v{n}.md 落盘、崩溃可恢复。
- **M2 可用性（+2 周）**：暂停 / 恢复、Goal-Check 决策树、震荡检测、prompt 模板编辑。
- **M3 打磨（+2 周）**：多 Session 管理、模型/参数配置、事件流富展示、签名打包分发。

## 14. 技术栈（已决策）

**Tauri v2 + Rust 后端 + React/TypeScript 前端**。

选型理由：
- 本项目最难的部分是**子进程编排**（长时运行的 `claude`/`codex`、流式 stdout、
  可取消、崩溃恢复、文件快照），Rust + `tokio` 是这类工作的最佳组合。
- Tauri 打包出的是单 binary、无运行时依赖，macOS 签名 / 公证流程干净。
- UI 是"输入框 + 开始按钮 + 事件流"，Web 栈实现最快；后续扩展设置页、
  prompt 模板编辑器、diff 查看器也直接用现成生态。
- 相比 SwiftUI：不绑定单一平台，且 Swift 写 async 子进程管理偏啰嗦。
- 相比 Electron：体积小一个数量级，冷启动更快。

模块划分（先登记，不在此 PRD 展开实现）：
- `cccplayer-core`（Rust crate）：状态机、持久化、harness 抽象、snapshot。
- `cccplayer-harness-claude` / `cccplayer-harness-codex`（Rust）：CLI 封装。
- `cccplayer-ui`（React）：三要素 UI + 事件流订阅 + 设置页。
- `cccplayer-app`（Tauri）：把以上三者粘起来，加 macOS 菜单、TCC 权限请求。

**Tauri v2 capabilities（M1 工程拆分必带）**：
- `core:default`、`shell:allow-execute`（限定可执行文件名为 `claude`、`codex`、
  `tar`、`zstd`，绝对路径白名单）。
- `fs:allow-read-recursive` / `fs:allow-write-recursive`：仅对用户已选目录与
  应用数据目录授权，**不**给全盘。
- `notification:default`：用于 §16.10 的走开通知。
- 不启用 `shell:default` 的"任意命令"能力——所有 shell 调用经 harness 走，
  Tauri 层不开后门。
- `tauri.conf.json` 的 `bundle.macOS.entitlements` 暂不要求沙箱（开发阶段免麻
  烦），分发包阶段再决定是否走 App Store 沙箱（M3+）。

## 15. 已关闭的问题（决策记录）

| # | 问题 | 决策 |
| --- | --- | --- |
| 1 | 技术栈 | Tauri v2 + Rust + React/TS（见 §14）。 |
| 2 | 目标如何传递给 agent | 工作目录下 `GOAL.md`，两个 agent 都读它（见 §9.3）。 |
| 3 | git 托管边界 | 不动用户 git，只用 tar+zstd 做快照（见 §8）。 |
| 4 | "目标已达成"判定 | 两票一致制：Claude Code 与 Codex 的 Goal-Check 都返回 `done` 才通过（见 §7、§11）。 |
| 5 | 并发 Session | 同时只允许一个活动 Session（见 §4）。 |

## 16. 工程决策（补充）

### 16.1 CLI 调用方式

- 不起可见 Terminal 窗口。Tauri 的 Rust 后端用 `tokio::process::Command` 直接 spawn
  `claude` / `codex` 子进程，stdin/stdout/stderr 走管道。
- 默认走两个 CLI 的一次性执行模式（类似 `claude -p "<prompt>"`、`codex exec`），
  并请求 stream-json 输出。**进程 exit 0 即本 turn 结束**，不自造心跳协议。
- 若实测发现某命令路径强依赖 TTY，用 `portable-pty` crate 分配伪终端，外部仍无
  可见窗口。
- 启动时跑一次 "CLI probe"：调 `--version` / `--help`，把支持的 flag 与输出模式
  固化到 `.cccplayer/cli-info.json`，后续调用据此选择参数——抵御 CLI 跨版本差异。

**子进程生命周期管理**（防止"主进程崩了 CLI 还在烧钱"）：
- spawn 时设独立进程组（`setsid` 或 `pre_exec` 调 `setpgid(0, 0)`），`kill_on_drop(true)`。
- 取消信号顺序固定：SIGINT → 5s → SIGTERM → 5s → SIGKILL，且作用于整个进程组。
- session.json 记录每个在途子进程的 `(pid, start_boottime)` 二元组。下次启动时
  发现遗留记录，**用 `(pid, start_boottime)` 双匹配**判断是否真是我们的进程，
  避免 pid 复用误杀。匹配上就回收，匹配不上就清理记录。

### 16.2 macOS PATH 与 preflight

- macOS GUI app **不继承** shell PATH，因此从 Finder 启动时 `/opt/homebrew/bin/claude`
  默认找不到。启动时按下列顺序探测 CLI：
  1. 用户在设置里填的绝对路径（最高优先）
  2. `/opt/homebrew/bin`、`/usr/local/bin`、`~/.local/bin`
  3. 进程继承的 `PATH`
- Preflight 面板在"开始"按钮旁给出逐项状态，**禁用开始按钮直到全部通过**：

  ```
  ● Claude Code CLI     未找到，请安装：<链接>
  ● Codex CLI           已安装 ✓
  ● Claude 登录状态     未登录，请在 Terminal 运行：claude login
  ● Codex 登录状态      已登录 ✓
  ● 工作目录            未选择
  ```

- **不代替用户登录**。CCCPlayer 不持有 API key、不弹密码框、不处理 OAuth 回调；
  一律指引用户在真实 Terminal 里跑 `claude login` / `codex login`，登录态通过 CLI
  自己在 `~/.claude`、`~/.codex` 下的 token 文件复用。
- **运行中失效的处理**：harness 在 stderr 中匹配常见认证失败字样 / HTTP 401，立即
  进入 `PAUSED` 状态并展示"请在 Terminal 运行 `<登录命令>` 后点恢复"。
- **两个 CLI 缺一不可**：preflight 任一项不通过即禁用"开始"。**不**提供单 agent
  降级模式——产品契约就是双方共识，降级会破坏这个契约。

### 16.3 输出解析鲁棒性

- Goal-Check 与 Verdict 要求 agent 以围栏代码块输出结构化数据。
- 解析器宽容：扫全文取第一个合法 JSON / YAML 块；找不到或解析失败时，**自动
  重试一次**，追问 "your last reply did not contain a valid JSON block, output
  only the JSON block this time"。两次都失败 → 本 turn 计入失败，进入 `ERRORED`
  等用户介入。

### 16.4 卡死检测（替代传统"超时预算"）

设计哲学是"烧算力、不烧用户时间"，因此**不设** token 预算，也**不设** turn / Session
的 wall-clock 硬超时。取而代之的是**无心跳检测**：

- 每个 harness 维护一个"最近事件时间戳"——只要 stream-json 发来任何事件（工具调
  用、文件读写、进度、token 结算），时间戳就刷新。
- 默认 10 分钟无新事件（设置里可调 1–60 分钟）→ 认为进程卡死：
  1. 先发 SIGINT 触发 CLI 自己的清理；5 秒后无退出再 SIGTERM；再 5 秒 SIGKILL。
  2. 回滚本 turn 到 Round 起始 snapshot，**自动重试一次**。
  3. 重试仍卡死 → 进 `ERRORED`，等用户介入。
- 事件流里展示 CLI 回报的 token 用量，纯信息性；不构成停止条件。
- 用户想要兜底时，可在设置里开启"Round 数硬帽"（默认关闭），或手动点暂停 / 停止。

**时钟选择（关键）**：
- 用 `CLOCK_MONOTONIC`（macOS / Linux 上睡眠期间停走）计算"距上次心跳过去多久"，
  **不要**用 wall-clock。否则用户合盖一晚再打开会立刻误判卡死。
- 同时订阅 `NSWorkspaceDidWakeNotification` / `NSWorkspaceDidSleepNotification`：
  唤醒时把心跳基准重置为"刚刚"，给 CLI 几分钟缓冲再重新计时。
- 这两条加上去，合盖、锁屏、网络断开恢复都不会触发误报。

**不做**的事：
- 不按 token 数停。
- 不按 Session 累计 wall-clock 停。
- 不按单 turn wall-clock 停（只要进程仍在正常推进事件，哪怕 2 小时也不干预）。

### 16.5 原子写、回滚与外部编辑冲突

- 所有 prompt 强制 agent 以 "写 tmp → rename" 完成文件写入。
- 每个 Round 启动前对工作树做 `tar + zstd` 快照落 `.cccplayer/snapshots/`。
- turn 被取消或崩溃时，harness 将该 turn 的所有文件改动回滚到本 Round 起始快照
  （不回滚到更早，避免损失已确认的进度）。

**用户外部编辑冲突防护**（针对 GOAL.md / PRD.md 这类共享文件）：
- 每 Round 起始时记录 GOAL.md、PRD.md 的 `mtime` + size + sha256（轻量）。
- 下一次 agent 准备改写 PRD.md 之前，harness 先做对比：若 mtime/sha256 与本
  Round 起始记录不一致，说明用户在过程中手工编辑过，**不静默覆盖**——弹窗
  让用户三选一：
  1. **以用户改动为准**：取消本 turn，把用户版本作为新基线，下一 turn 重跑。
  2. **以 agent 即将写入的版本为准**：覆盖（用户改动会进 round 起始快照里，
     可手工恢复）。
  3. **取消并暂停**：进 PAUSED，让用户决定。
- GOAL.md 的处理更严格：被外部编辑（含删除）→ 立刻 `PAUSED`，不允许 agent 把
  目标"漂"过去。GOAL.md 是 Session 级不可变契约。

**GOAL.md 删除监测**：每个 turn 启动前 harness 强制读一次 GOAL.md。文件不存在
→ 立刻 `PAUSED`，提示"目标文件丢失，请恢复或新建 Session"。

### 16.6 模型与推理参数

- 设置页暴露两个选择：
  - Claude 模型：默认 `claude-opus-4-7`（最强）。
  - Codex reasoning level：默认 `high`。
- 通过 CLI flag 传入，不劫持 CLI 自身的配置文件。

### 16.7 用量显示（M1 范围）

M1 只做最简单的一档：**本 Session 累计 token 用量**，从 stream-json 事件里累加。

- 底部状态条挂一个小组件："本 Session · <claude_in+out> tokens · <codex_in+out> tokens"。
- 数据源头：两个 harness 订阅 CLI 自己回报的 usage 事件；写入 `.cccplayer/usage.json`
  落盘，Session 恢复后继续累加。
- **M1 不做** 5 小时滚动额度 / 周度余额显示。这两项依赖 CLI 是否提供非交互查询
  命令，等真机探测稳定后再作为增量（M3+）加入。找不到稳定接口就老实标"不可用"，
  不伪造数字。
- 用量数字**不**参与任何停止条件，纯展示。

### 16.8 Turn 结束分类与重试策略

每个 turn 结束（CLI 退出 / 被 cancel）时，harness 必须对结果做分类，决定下一步：

| 结果分类 | 判别条件 | 处理 |
| --- | --- | --- |
| `ok` | exit 0 + 产出物符合预期（见下方"产出物预期"） | 进入下一态 |
| `output_malformed` | exit 0 但产出物缺关键段 / 关键文件缺失或空 | 走 §16.3 的 retry-with-clarification，最多 1 次；再不行 → `ERRORED` |
| `stalled` | 心跳超时（见 §16.4） | 自动 cancel + retry 1 次；再卡 → `ERRORED` |
| `crashed` | 非零 exit 且非认证错误 | retry 1 次；再失败 → `ERRORED` |
| `auth_failed` | stderr 命中认证模式 | 不 retry → `PAUSED`，引导用户登录 |
| `refused` | exit 0 + 工作树无文件改动 + 输出命中拒绝关键词（"I can't help"、"I won't"、"unable to"、"refuse" 等，全词或开头匹配） | 不 retry → `ERRORED`，提示用户修改目标 |

判定顺序：`auth_failed` > `refused` > `stalled` > `crashed` > `output_malformed` > `ok`。

**各阶段产出物预期**（用于 `ok` / `output_malformed` 判定）：

| 阶段 | 必须存在 | 必须包含 |
| --- | --- | --- |
| PLANNING | `PRD.md` 非空 | §17.1 列的 7 个一级 heading 全在；Goal 节非空 |
| IMPLEMENTING | （只看是否有合理工作树改动） | 至少一个非 `.cccplayer/` 下的文件被新建/修改；exit summary 里出现 "files:" |
| REFINING | 最新 `codex_review_v{N}.md` 末尾出现 `## Claude Code 回应` 段 | 该段下至少一个 `### …` 子节 |
| REVIEWING | 新增 `codex_review_v{N+1}.md` | `## Verdict` 段存在且 `status:` 为三种合法值之一；`approved` 时 `blocking:` 必须空 |
| GOAL-CHECK | 一段 fenced JSON | 含 `done`/`missing`/`next_state`/`rationale` 四字段，类型正确 |

### 16.9 暂停语义

"暂停"按钮在不同时刻含义不同，必须明确：

| 暂停时机 | 行为 |
| --- | --- |
| Turn 进行中 | 走 cancel 协议（SIGINT→SIGTERM→SIGKILL），**回滚本 turn 到 Round 起始 snapshot**，记录"暂停在 phase X、Round N、turn T"。恢复时从该 turn 起点重跑。 |
| Turn 之间（短暂的状态机过渡） | 仅记录状态，不回滚。恢复时直接进下一个 turn。 |
| PLANNING 第一个 turn（Round 1） | 同上 cancel + 回滚到 round-00 snapshot；恢复后从头 PLANNING。这意味着 PLANNING 中途暂停**会丢弃**到目前为止的 PRD 草稿。UI 必须明示这一点，不要静默丢工作。 |

恢复（点"开始"）时，§7 的决策树先跑 GOAL-CHECK，可能直接判定已 DONE 或换状态，
不一定回到原暂停点——这是设计上有意如此（万一外部状态变了）。

### 16.10 应用数据目录、Prompt 模板覆盖与 Agent 工具自动批准

**应用数据目录**（区分于工作目录）：
```
~/Library/Application Support/CCCPlayer/
├── settings.json     # 全局设置：CLI 绝对路径、模型、心跳超时、Round 硬帽…
├── prompts/          # 用户自定义 prompt 模板，覆盖内置默认
│   ├── planning.md
│   ├── implementing.md
│   ├── refining.md
│   ├── reviewing.md
│   └── goal-check.md
└── logs/             # 应用自身（非 Session）日志
```
- 设置面板对外是 GUI；底层就是这份 `settings.json`，可手工编辑。
- `prompts/` 任一文件存在则覆盖内置默认；缺失则用应用包内默认。设置页有
  "重置该模板为默认"按钮和"重新加载"按钮（不需要重启）。

**Agent 工具调用自动批准**（M1 关键决策）：
- Claude Code 与 Codex 默认会在执行危险操作前请用户确认（写文件、跑 shell 命
  令、改 git…）。在 CCCPlayer 的无头编排里没人确认，循环会**永久阻塞**。
- 因此 harness 调用两个 CLI 时，必须传入"自动批准全部工具调用"的等效参数
  （Claude Code 是 `--dangerously-skip-permissions` 一类，Codex 类似；具体名称
  M1 CLI probe 时确定）。
- **作用域 = 工作目录内**：通过 CLI 自带的目录限定参数（如 `--add-dir <workdir>`）
  把 agent 的可写边界限定在工作目录里，**不允许 agent 操作工作目录之外的文件**。
- 这是有意识的权限取舍——产品卖点就是"无人值守"。UI 必须在首次启动 / 设置页
  明示："CCCPlayer 会让两个 agent 在你选定的工作目录内自由读写、执行命令；
  请确保选择的目录里没有不能丢的关键资产，并保留备份。"

**走开通知**：
- 进入 `DONE` / `ERRORED` / `PAUSED` 这三个用户需要回到电脑前的状态时，发一条
  macOS 系统通知（`tauri-plugin-notification`）。
- 通知点击后跳回应用主窗。
- 设置里可关闭。

### 16.11 工作目录失联

工作目录所在卷（外接盘、网络盘、远程挂载）随时可能消失。harness 的所有写操作
若返回 `ENOENT` / `EIO` / `EROFS`：

- 立刻进 `PAUSED`，不重试（重试会刷大量错误日志）。
- UI 显示"工作目录已失联：`<path>` 不可访问。请确认设备已挂载后点恢复"。
- 恢复时先做一次目录探活（写一个 `.cccplayer/.alive` 临时文件并删除），通过才进
  下一态。

## 17. Prompt 模板（待你确认）

下面五个 prompt 作为 M1 起点；全部以英文撰写（两个 CLI 对英文指令最稳），文件
内部的固定段名若 PRD 指定为中文则保留中文。执行时由 harness 做模板填充（目录路
径、Round 序号、上一版本号）。

### 17.1 PLANNING（Claude Code）

```text
You are working in {workdir}. First read GOAL.md — that is the user's
immutable goal for this session; never modify it.

IMPORTANT: the working directory may already contain the user's existing
code — possibly a real in-progress project. Before writing any design,
SURVEY the current state:
  - List top-level files and directories.
  - Identify language, build system, entry points, and key modules.
  - Note what is already implemented vs. what the goal still requires.
  - Never assume an empty workspace; never plan to scrap existing code
    unless GOAL.md explicitly requires it.

Task for this turn: produce or update PRD.md so it fully specifies how to
reach GOAL.md FROM THE CURRENT STATE. PRD.md is the single design document;
later turns will implement code against it.

Required top-level headings (in order):
1. Goal            — verbatim restatement of GOAL.md, one sentence.
2. Current state   — your survey findings. Write "empty workspace" if the
                     directory is empty; otherwise 3–10 bullets covering
                     stack, entry points, what is already done, and any
                     obvious gaps or risks inherited from existing code.
3. Scope           — what is in, phrased as *delta* over Current state.
4. Non-goals       — what is explicitly out.
5. Design          — architecture, key modules, file layout, data model,
                     external dependencies. Where existing code already
                     fits the design, say "reuse as-is" rather than
                     redesigning.
6. Milestones      — ordered checklist of shippable increments, starting
                     from Current state and ending at GOAL.md.
7. Open Questions  — anything you could not decide; empty list is fine.

If PRD.md already exists, revise it in place; preserve prior decisions
unless newly contradicted. If codex_review_v*.md files exist, read the
highest-numbered one and fold any PRD-level feedback into this revision.

Write PRD.md atomically (temp file + rename). Do not modify any other file
in this turn.

When done, print one line to stdout:
    PLANNING done: <N> sections, +<added>/-<removed> lines
```

### 17.2 IMPLEMENTING（Claude Code）

```text
You are working in {workdir}. GOAL.md is the user's goal (read-only). PRD.md
is the design you must follow.

Task for this turn: make concrete, working code changes that advance the
next unchecked milestone in PRD.md. Do NOT reopen PRD-level decisions in
this turn; if you genuinely must, stop after appending a note under
"Open Questions" in PRD.md.

Rules:
- Favor small, compilable, testable increments over large refactors.
- Add tests when the milestone implies them.
- Run the project's build / test command if one exists; report its result.
- Never touch GOAL.md or any codex_review_v*.md file.
- Every file write must be atomic (temp file + rename).

When done, print to stdout:
    IMPLEMENTING done: <milestone title>
    files: <comma-separated paths>
    build: <ok | failed | n/a>   tests: <passed/total | n/a>
```

### 17.3 REFINING（Claude Code）

```text
You are working in {workdir}. Read, in order:
  1. GOAL.md (read-only).
  2. PRD.md.
  3. codex_review_v{N}.md — the highest-numbered review file.
  4. If present, a "## Goal-Check 待办" section appended to that same review
     file by the orchestrator — this lists items that BOTH agents agreed
     are still missing for the goal, even though the latest verdict was
     "approved" on code quality. Treat each such item with the same
     severity as a blocking review finding.

That review ends with a "## Verdict" section listing blocking and optional
non_blocking items. Combine that list with any "Goal-Check 待办" items as
your full work set for this turn.

Task for this turn:

A. Address every blocking item AND every Goal-Check 待办 item. For each,
   either fix it (in code and/or PRD.md) or reject it with a specific
   technical reason.

B. Consider non_blocking items; act on them only when clearly beneficial.

C. Append (do not overwrite) a "## Claude Code 回应" section to the SAME
   codex_review_v{N}.md file, with one subsection per blocking item:

       ### <verbatim blocking item title>
       - status: accepted | partial | rejected
       - action: <what changed, with file paths>   (omit if rejected)
       - reason: <why this resolves the item, or why rejected>

Rules:
- Never modify earlier codex_review_v*.md files.
- Never edit GOAL.md.
- All file writes must be atomic.

When done, print to stdout:
    REFINING done on v{N}: accepted <X>, partial <Y>, rejected <Z>
    files touched: <comma-separated paths>
```

### 17.4 REVIEWING（Codex）

```text
You are working in {workdir} as an independent reviewer. Read:
  1. GOAL.md — the immutable user goal.
  2. PRD.md — the current design. Note its "Current state" section: some
     code in the working tree was authored by the user before this session
     started. Review it for goal-fit, but do not flag pre-existing style
     issues as blocking unless they actively prevent the goal.
  3. The working tree source files.
  4. All prior codex_review_v*.md files — do not repeat points already
     marked "accepted" in their "## Claude Code 回应" sections unless they
     have since regressed.

Pick N = max existing version + 1 (or 1 if none). Create
codex_review_v{N}.md with exactly this structure:

    # Codex Review v{N}
    date: <YYYY-MM-DD>
    goal: <one-line restatement of GOAL.md>

    ## Summary
    <2-4 sentences: what was built, what is missing or risky>

    ## Strengths
    <bullets; skip section if none>

    ## Findings
    ### <short finding title>
    - severity: blocking | non_blocking
    - where: <file:line or "design-level">
    - detail: <one paragraph>
    - suggestion: <concrete change>
    (repeat per finding)

    ## Verdict
    - status: approved | changes_requested | blocked
    - blocking:
      - <verbatim titles of blocking findings, one per line; empty list
         if none>
    - non_blocking:
      - <verbatim titles of non_blocking findings>

Rules:
- Modify no file other than your new codex_review_v{N}.md.
- `status: approved` is only correct when the blocking list is empty.
- Write the file atomically.

When done, print to stdout:
    REVIEW done: v{N}, verdict <status>, blocking <count>
```

### 17.5 GOAL-CHECK（Claude Code 与 Codex 各一次，只读）

```text
You are performing a READ-ONLY evaluation. Do not edit any file. Do not run
any command that mutates state beyond reads.

Read:
  1. GOAL.md.
  2. PRD.md, if present.
  3. The highest-numbered codex_review_v*.md, if any.
  4. The working tree file list and a concise summary of recent changes.

Decide whether the user's goal in GOAL.md is fully delivered by the current
working tree. Err on the strict side: if any part of the goal is incomplete,
untested, or unverifiable, mark it not done.

Output exactly one fenced JSON block and nothing else outside it:

    ```json
    {
      "done": true | false,
      "missing": ["<short items>"],
      "next_state": "DONE" | "PLANNING" | "IMPLEMENTING" | "REFINING",
      "rationale": "<one sentence>"
    }
    ```

next_state rules:
- "DONE" iff done=true.
- "PLANNING" if PRD.md is missing or materially inconsistent with GOAL.md.
- "REFINING" if the latest codex_review_v*.md Verdict has unresolved
  blocking items.
- "IMPLEMENTING" otherwise.
```

---

## 18. 决策记录（累计）

| # | 问题 | 决策 | 见 |
| --- | --- | --- | --- |
| 1 | 技术栈 | Tauri v2 + Rust + React/TS | §14 |
| 2 | 目标如何传递给 agent | 工作目录下 `GOAL.md` | §9.3 |
| 3 | git 托管边界 | 不动用户 git，只用 tar+zstd 快照 | §8 |
| 4 | "目标已达成"判定 | 双方 Goal-Check 都 `done=true` 才通过 | §7、§11 |
| 5 | 并发 Session | 同时仅一个活动 Session | §4 |
| 6 | CLI 调用方式 | 直接 spawn + stream-json，必要时 PTY 兜底 | §16.1 |
| 7 | 登录态 | 依赖 CLI 自有 token；preflight 引导，不代劳 | §16.2 |
| 8 | PATH 陷阱 | 固定路径探测 + 用户可填绝对路径 | §16.2 |
| 9 | Goal-Check 解析 | 围栏 JSON + 宽容解析 + 单次重试 | §16.3 |
| 10 | 预算 | 不设 token / wall-clock 上限；只做无心跳卡死检测（默认 10 分钟） | §16.4、§11 |
| 11 | 原子写与回滚 | prompt 强制 tmp+rename，turn 取消回滚到 Round 起始快照 | §16.5 |
| 12 | 模型 | 默认 Claude Opus 4.7 + Codex reasoning=high | §16.6 |
| 13 | 进度显示 | 状态条 + 事件时间线 + 原始日志抽屉 | §6.1 |
| 14 | 用量显示 | M1 只显示本 Session 累计 token；滚动 / 周度余额延后，待 CLI 能力探测后再做 | §16.7 |
| 15 | 非空工作目录 | 支持既有代码：PLANNING 先盘点、PRD 含 "Current state"、`round-00` 保留原始快照 | §2.1、§10、§17.1 |
| 16 | 心跳时钟 | 用 CLOCK_MONOTONIC + macOS 唤醒通知，避免合盖误判卡死 | §16.4 |
| 17 | 子进程治理 | 进程组 + kill_on_drop + (pid,start_boottime) 双匹配防 pid 复用 | §16.1 |
| 18 | 落盘脱敏 | transcripts 与 events.log 写盘前先做正则脱敏 | §12 |
| 19 | GOAL-CHECK 触发与并行 | 入口串行、REVIEWING 末尾并行；其他时刻不跑 | §5 |
| 20 | session 文件锁 + schema 版本 | flock 互斥单实例；session.json 带 schema_version=1 | §8 |
| 21 | 进展停滞 | missing 列表大小连续 3 轮不严格下降视作停滞 → ERRORED | §11 |
| 22 | Agent 拒绝识别 | 关键词 + 无文件改动检测；不重试，提示改目标 | §11、§16.8 |
| 23 | 单 agent 不可用 | hard-stop，不提供降级模式 | §16.2 |
| 24 | 首次启动 / 切换目录 | 空白态欢迎页 + preflight；活跃 Session 中禁切目录 | §6.2、§6.3 |
| 25 | Turn 结果分类 | 6 种结果 + 明确判定顺序 + 各自重试策略 | §16.8 |
| 26 | 暂停语义 | 中途暂停 cancel+回滚；turn 间暂停仅记录；PLANNING 中途暂停会丢稿（明示） | §16.9 |
| 27 | 用户外部编辑冲突 | mtime+sha256 对比，PRD.md 三选一弹窗，GOAL.md 严格不允许漂移 | §16.5 |
| 28 | GOAL.md 删除监测 | 每 turn 启动前重读，丢失即 PAUSED | §16.5 |
| 29 | `.gitignore` 自动追加 | Session 创建时若有 `.git/` 则追加 `.cccplayer/` | §8 |
| 30 | 应用数据目录 | `~/Library/Application Support/CCCPlayer/`：settings + prompts 覆盖 + logs | §16.10 |
| 31 | Agent 工具自动批准 | 传 `--dangerously-skip-permissions` 类参数 + 用 CLI 自带目录限定，无人值守的代价 | §16.10、§12 |
| 32 | 走开通知 | 进 DONE/ERRORED/PAUSED 时发 macOS 系统通知 | §16.10 |
| 33 | 工作目录失联 | ENOENT/EIO 立即 PAUSED，恢复时探活 | §16.11 |
| 34 | 各阶段产出物预期 | 表格化各 turn 的"合格输出"判据，喂给 §16.8 分类器 | §16.8 |
| 35 | REFINING 兼容 Goal-Check 来源 | 当 verdict=approved 但 Goal-Check 不通过时，写 `## Goal-Check 待办` 段供 REFINING 当 blocking 处理 | §17.3 |
| 36 | 零遥测 | 产品级承诺，无开关 | §12 |
| 37 | TCC 拒绝处理 | 引导到系统设置 + 重试按钮 | §12 |
| 38 | Tauri capabilities | 最小可执行白名单 + 仅授权工作目录读写 + 通知；不开 shell 后门 | §14 |

---

## 19. 已知事项 / 后续优化（不阻塞 M1）

明确写下来，免得日后当成"漏了"：

1. **快照磁盘膨胀**：N 个 Round × 几十 MB-几 GB 可能累计很大。M2+ 引入保留策略
   （永远保留 `round-00` 与最近 5 份；中间的延迟 GC）。
2. **大型现有代码库 PLANNING 爆 context**：先依赖 Claude Code 自身的上下文管理；
   PRD 的 Current state 节若被截断，至少 PRD 应记一句"survey 不完全"。M2 考虑分
   层 survey。
3. **Fake CLI 测试套件**：M1 之内做出 `cccplayer-fake-claude` / `cccplayer-fake-codex`
   两个可执行 fixture，能按预设脚本吐 stream-json。集成测试和回归测试都用 fake，
   不烧真实 token。**这一项虽列在"已知事项"里，但开发阶段必做**——M1 工程拆分
   时一并安排。
4. **Schema 迁移脚本**：`session.json` 字段升级时的迁移函数。第一次破坏性升级前
   不必预先建框架。
5. **滚动 / 周度配额显示**：见 §16.7。M3+ 视 CLI 能力。
6. **events.log 滚动**：单 Session 跑久会膨胀到几十 MB。M2 加按大小切片归档。
7. **目标文本中的 prompt injection**：用户输入的目标会原样进 GOAL.md 与 prompt。
   不专门防御——两个 agent 自身的 guardrail 兜底；CCCPlayer 不试图做内容审查。
8. **CCCPlayer 自指开发**：用 CCCPlayer 来开发 CCCPlayer 时，工作目录必须指向
   一份**独立的源码副本**，不能指向当前正在运行的 binary 所在目录。README 与
   首次启动提示里写明。
9. **PRD/Review 文档自身膨胀**：PRD.md 多轮修订后可能很大；Goal-Check 把整份
   PRD + 最新 review 喂回 agent，可能逼近 CLI 上下文上限。M2 加"摘要节"或"差量
   评估"机制；M1 暂时信任 CLI 自己的截断策略。
10. **Gatekeeper 首次启动**：未签名版本 macOS 默认拒绝运行。M1 开发用户右键
    "打开"绕过；M3 完成签名 + 公证。
11. **键盘快捷键 / 无障碍**：M2 加 Cmd+Enter / Esc 等基本快捷键与 VoiceOver。

---

*下一步：进入 M1 的工程拆分——仓库骨架、状态机骨架、harness 接口、fake CLI、
以及一个能跑通 "PLANNING → IMPLEMENTING → REVIEWING" 最小回路的 demo。*
