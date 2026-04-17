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

## 2. 目标用户 & 前置条件

- 目标用户：在 macOS 上已配置好 Claude Code 与 Codex CLI 的开发者。
- 前置条件（应用启动时检查）：
  1. 系统：macOS 13+（Apple Silicon / Intel 皆可）。
  2. `claude` CLI 已安装、已登录（能以非交互方式执行）。
  3. `codex` CLI 已安装、已登录。
  4. 用户已选定一个**工作目录**（project workspace），应用拥有读写权限。
  5. `git` 可用（用于快照与回滚，详见 §8）。

不满足时，应用以"前置检查"页面引导用户逐项解决，而非在运行中失败。

## 3. 用户故事

- **US-1 一键启动**：我输入"做一个能在本地跑的 Markdown TODO CLI，用 Rust 写"，
  点"开始"，回来吃个饭，期望看到一个可运行的雏形 + 完整 PRD + 若干轮 Codex 评审。
- **US-2 意外中断**：跑到一半我锁屏 / 关机 / Kill 掉 Terminal。回来再点"开始"，
  应用应判断"目标是否已完成"，没完成则从最近一次稳定状态继续，不重头跑。
- **US-3 手动干预**：我看到某一版 `codex_review_v3.md` 指出一个安全问题，我想自己
  改两行代码再继续。暂停 → 编辑 → 继续，循环从当前工作树重新进入评审。
- **US-4 终止**：我发现目标定偏了，点"停止"。再次"开始"时，应用询问是继续旧目标、
  开新会话，还是基于旧工作树换新目标。

## 4. 核心概念与术语

| 术语 | 含义 |
| --- | --- |
| Goal | 用户输入的自然语言目标（不可变，除非新建会话）。 |
| Session | 一次"目标→完成"的端到端过程，对应一个工作目录 + 一份持久化状态。 |
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
- 事件流点击任意条目跳转到对应文件/日志。
- 设置页：模型选择、每 Round 超时、最大 Round 数、自定义 prompt 模板。

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

"目标是否已达成"的判断统一走一个**只读 Claude Code 调用**，输入是
`Goal + PRD.md + 最新 codex_review_v{n}.md + 工作树 diff 摘要`，输出
结构化 JSON：

```json
{
  "done": false,
  "missing": ["未实现 CLI 的 --export 参数", "无集成测试"],
  "next_state": "IMPLEMENTING"
}
```

## 8. 持久化与恢复

每个 Session 在工作目录下维护 `.cccplayer/`：

```
.cccplayer/
├── session.json          # Goal、状态机、Round 计数、时间戳
├── events.log            # 追加式事件流（事件流 UI 的数据源）
├── transcripts/
│   ├── round-01-impl.txt
│   ├── round-01-review.txt
│   └── …
└── snapshots/
    └── round-01/         # git stash 或轻量 tar，用于快速回滚
```

工作目录本体由 CCCPlayer 托管的 `git` 仓库自动提交（每个 Round 一次 commit，
message 形如 `cccplayer: round 3 implement`）。

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
| PLANNING | Goal | 新建或更新 `PRD.md` |
| IMPLEMENTING | Goal、`PRD.md` | 代码改动（由 Claude Code 直接写盘） |
| REFINING | `codex_review_v{n}.md` | 在同一文件尾部追加 `## Claude Code 回应` 段，并修改代码 |
| GOAL-CHECK（只读） | Goal、PRD、最新评审、diff 摘要 | 结构化 JSON |

### 9.2 Codex 的职责

| 阶段 | 输入 | 输出 |
| --- | --- | --- |
| REVIEWING | Goal、`PRD.md`、工作树 | 新建 `codex_review_v{n+1}.md`，末尾必须含 verdict 段 |

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

- `PRD.md`：由 Claude Code 全权维护。结构要求（由 prompt 约束）：
  `Goal → Scope → Non-goals → Design → Milestones → Open Questions`。
- `codex_review_v{n}.md`：
  - 由 Codex 创建，单调递增，**不允许删除旧版本**。
  - Claude Code 在 `REFINING` 阶段追加 `## Claude Code 回应` 段，说明每条
    blocking 项的处理（接受 / 部分接受 / 拒绝+理由）。
  - 文件末尾必须有 `## Verdict` 段。
- 两份文档都进入 git 历史，方便审阅演进过程。

## 11. 停止条件

Round 循环的终止满足**任一**即可：

1. 最近一轮 Codex verdict == `approved` **且** Goal-Check 判定 `done == true`。
2. 达到最大 Round 数（默认 20，可配置），进入 `ERRORED`，等待用户决策。
3. 用户显式暂停 / 停止。
4. 连续 2 轮评审内容高度相似（haiku 做相似度判断）→ 判定"震荡"，进入 `ERRORED`。

## 12. 安全与权限

- 仅在用户选定的工作目录内读写；应用自身目录与系统目录访问走 macOS 的 TCC。
- 不代替用户登录 CLI；不持有 API key。
- 运行期默认**离线文档可见**、**联网由 CLI 本身决定**；应用不额外外联。
- 事件流中对可能包含密钥的行做基础脱敏（`sk-…`、`ghp_…` 等正则）。

## 13. 里程碑

- **M1 MVP（2 周）**：单 Session、顺序 Round、UI 三要素（输入 / 开始 / 事件流）、
  Claude Code + Codex 打通、PRD.md 与 codex_review_v{n}.md 落盘、崩溃可恢复。
- **M2 可用性（+2 周）**：暂停 / 恢复、Goal-Check 决策树、震荡检测、prompt 模板编辑。
- **M3 打磨（+2 周）**：多 Session 管理、模型/参数配置、事件流富展示、签名打包分发。

## 14. 待回答的问题（Open Questions）

1. **技术栈选型**：SwiftUI 原生 vs Tauri/Electron？原生体验更好但与 Node/Rust 生态的
   harness 实现要多一层 bridge。
2. **Claude Code 如何"读取用户目标"**：作为 prompt 注入，还是写成工作目录下的
   `GOAL.md`？倾向后者——更透明、便于 Codex 同样读取。
3. **Git 托管边界**：如果工作目录已经是用户的 git 仓库，CCCPlayer 的自动 commit
   如何与用户自己的提交共存？倾向"另建一个 `cccplayer/*` 分支线"，或完全只用
   stash + 快照文件，不动用户的主分支。
4. **"目标已达成"的判定误差**：Claude Code 自评容易高估。是否引入 Codex 做第二
   意见？（会多一次 CLI 调用，成本上升。）
5. **并发 Session**：MVP 阶段明确只支持一个活动 Session，避免并发争抢同一工作
   目录与模型配额。

---

*下一步建议：先就 §14 里的 1、3、4 做决策，再进入 M1 的工程拆分。*
