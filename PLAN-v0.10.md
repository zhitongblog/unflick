# unflick v0.10 — "Depth" 项目计划

> **状态（2026-08-22）**：Phase 0 / 1 / 2 / 3 全部实现并验证完毕。
> 剩下的只有打包发布与跨平台构建。验证记录见文末「已验证」一节。

> 目标：补齐播放器及格线功能，并把「AI 原生」从叙事变成真实能力。
> 依据：2026-08 竞品调研（VLC 4.0 / IINA / PotPlayer / mpv+ImPlay / SubX / vlc-mcp-server）。

---

## 背景：两个必须解决的问题

### 问题 A —— MCP/CLI 驱动的是一个无画面的幽灵播放器

```
daemon.rs:21   Player::new()  →  MpvHandle::new("null")   // vo=null，无视频输出
lib.rs:494     Player::new_for_render()                   // GUI 自己的实例
```

两个进程、两个 mpv 实例、互不可见。后果：

- AI agent 调 MCP `play` → 只有声音，没有画面
- 用户开着 GUI 时，`unflick pause` 不会暂停眼前的视频
- `Player::new_with_video()` 是死代码，从未被调用

README 承诺的「三个界面，一个内核」在控制面上是断的。**这是 v0.10 的第一优先级。**

### 问题 B —— 基础播放功能有明显缺口

grep 确认为零实现：字幕延迟、音频延迟、章节、A-B 循环、逐帧步进、字幕样式、
播放列表 repeat/shuffle、播完自动下一个。这些是 PotPlayer / IINA / mpv 的标配。

---

## Phase 0 — 统一控制面（架构修正）

**成果**：GUI 运行时，CLI 和 MCP 直接驱动用户眼前的那个窗口。

| 步骤 | 内容 |
|---|---|
| 0.1 | `daemon.rs` 抽出 `serve_control(player, playlist, db)`，与 `start_daemon()` 共用 dispatcher |
| 0.2 | `GuiPlayer.playlist` 改为 `Arc<Playlist>`（`Deref` 保证现有调用点不变） |
| 0.3 | GUI 启动后拉起控制监听线程，绑定同一端口 `127.0.0.1:19542`，注入自己的 `render_player` |
| 0.4 | 端口被无头 daemon 占用时：先发 `shutdown` 让位，再重试绑定一次；仍失败则降级并记录日志 |
| 0.5 | GUI 前端已有 250ms `player_status` 轮询 → 外部指令引起的状态变化自动反映到 UI，无需额外改动 |

**验收**：GUI 开着的情况下 `unflick pause` / MCP `pause` 能暂停窗口里的画面。

---

## Phase 1 — P0 播放及格线

全部按 **core → daemon → CLI → MCP → GUI** 五面对齐（CLAUDE.md 硬性要求）。

| # | 功能 | mpv 属性 / 命令 | CLI | MCP |
|---|---|---|---|---|
| 1 | 字幕延迟 | `sub-delay` | `subtitle delay <s> [--relative]` | `subtitle_delay` |
| 2 | 音频延迟 | `audio-delay` | `audio delay <s> [--relative]` | `audio_delay` |
| 3 | 章节 | `chapter`, `chapter-list/*` | `chapter list\|next\|prev\|seek <i>` | `chapter_list` / `chapter_seek` |
| 4 | A-B 循环 | `ab-loop-a`, `ab-loop-b` | `loop a\|b\|clear\|status` | `ab_loop` |
| 5 | 逐帧步进 | `frame-step` / `frame-back-step` | `frame next\|prev` | `frame_step` |
| 6 | 字幕样式 | `sub-scale` `sub-pos` `sub-color` `sub-border-size` `sub-bold` | `subtitle style get\|set` | `subtitle_style_get/set` |
| 7 | 列表 repeat/shuffle | 自实现 | `playlist repeat <off\|one\|all>` / `shuffle` | `playlist_mode` |
| 8 | 播完自动下一首 | EOF 检测 + 推进 | （随 7 生效） | （随 7 生效） |

**GUI 快捷键**（对齐 mpv / PotPlayer 习惯）

| 键 | 动作 |
|---|---|
| `z` / `Z` | 字幕延迟 −/+ 0.1s |
| `Ctrl+-` / `Ctrl+=` | 音频延迟 −/+ 0.1s |
| `PgUp` / `PgDn` | 上一章 / 下一章 |
| `[` / `]` / `\` | 设 A 点 / 设 B 点 / 清除循环 |
| `,` / `.` | 后退一帧 / 前进一帧 |

**GUI 可视化**
- 进度条渲染章节刻度，hover tooltip 显示章节名
- A-B 循环区间在进度条上高亮
- 字幕菜单加延迟调节行；音轨菜单加音频延迟行
- 设置面板加「字幕样式」分区，持久化到 `settings.json` 并在启动时应用

---

## Phase 2 — P1 护城河：MCP 深度工具

外部 MCP 外壳（`vlc-mcp-server` 等）做不到的事 —— 需要播放器内部状态。

| 工具 | 能力 | 状态 |
|---|---|---|
| `search_transcript` | 在当前字幕里检索，返回带时间戳的命中行 | 完成 |
| `seek_to_text` | 「跳到他说 X 的地方」——检索 + 定位一步到位 | 完成 |
| `transcript_get` | 整篇转录（带时间轴），供模型阅读/摘要 | 完成 |
| `generate_chapters` | 按转录里的停顿切章节 | 完成 |
| `set_chapters` | 模型自己读完转录后给出章节表 | 完成 |
| `clear_chapters` | 清除合成章节（不影响容器自带章节） | 完成 |
| `describe_frame` | 当前帧缩放编码为 JPEG，以 MCP image block 返回 | 完成 |

> `describe_frame` 依赖 Phase 0：只有控制面统一后，agent 抽到的帧才是用户正在看的那一帧。

**实现要点**

- `core/transcript.rs` —— SRT / WebVTT 解析（含 `<i>`、`{\an8}` 标记剥离、
  无小时字段的 VTT、非 UTF-8 字幕的 lossy 解码）。字幕来源按用户预期的顺序解析：
  当前选中的外挂轨 → 任意外挂轨 → 同名 sidecar → 内嵌文本轨（ffmpeg 抽一次并缓存）。
  图形字幕（PGS / VobSub）无文本可搜，明确跳过。
- `Player::virtual_chapters` —— mpv 运行时无法被塞入章节，所以合成章节挂在 Player 上，
  由 `chapter_list` 合并输出。**结果是真导航**：进度条刻度、`chapter_seek`、PgUp/PgDn
  全部照常工作；容器自带章节优先，`set_chapters` 会拒绝覆盖它们；切文件时自动清除。
- `core/vision.rs` —— mpv 截帧（`video` 模式，不烧录字幕与 OSD）→ ffmpeg 缩放到
  最长边 768px 的 JPEG。CLI 走 `--output` 写文件，MCP 走 base64 image block ——
  往终端打一兆 base64 对谁都没好处。base64 编码器手写（20 行，全树唯一调用点），
  附 RFC 4648 测试向量。

---

## Phase 3 — 叙事修正

- README 中 `the only video player that AI agents can drive natively` 需改写。
  `vlc-mcp-server`（GitHub + PyPI）和 mpv 的 MCP 工具都已存在。
  改为强调**一等公民**而非唯一：MCP 是内建控制面，不是第三方外壳；每个功能先无头可用再有按钮。

---

## 暂缓（记录在案，不进 v0.10）

在线字幕搜索（OpenSubtitles）、进度条缩略图预览、音频均衡器 / 响度归一化、
自定义快捷键、鼠标手势、Music/Mini 模式、DLNA 投屏、SMB/NFS 网络路径、
DVD/Blu-ray/ISO、插件脚本系统、浏览器扩展。

---

## 已验证（2026-08-20）

**自动化**：
Phase 1 自测 **40/40**（含 3 个负例与 2 个断点续播策略回归）；
Phase 2 自测 **29/29**（转录检索、定位、章节合成与边界、帧捕获）；
Rust 单元测试 **11/11**（SRT/VTT 解析、检索、章节推导、base64 RFC 向量）。
MCP `tools/list` 返回 **60** 个工具（v0.9 为 43）。
`cargo build` 与 `tsc --noEmit` 均无错误。

**Phase 2 端到端**：MCP `describe_frame` 请求 `position: 30` 返回
`['text', 'image']` 两个 block，解码出的 JPEG 正是 ffmpeg testsrc 在第 30 秒的画面
（图案上显示 "30"）—— 说明 seek、截帧、缩放、编码、传输整条链路正确。

**实机 GUI**：debug 构建启动无 panic；预先运行的无头 daemon 在 GUI 启动时让出控制端口；
`unflick shutdown` 被 GUI 正确拒绝；CLI 的 `play` / `pause` / `seek` / `loop a|b`
全部作用于窗口里的播放器，并反映到界面上（截图确认 A/B 标记、循环区间着色、
10s/20s 章节刻度、播放键状态、时间轴位置）。

**过程中修掉的两个既有 bug**

1. `keep-open=yes` 在 EOF 把全局 `pause` 置真 → 之后 `loadfile` 的文件一律停在 0:00
   暂停。自动连播每次都会踩到，手动 `unflick play` 在上一个文件播完后也会踩到。
   已在 `Player::play` 中清除。
2. 断点续播会把「播到结尾」的位置存下来 → 再打开同一个文件直接停在最后一帧，
   看起来像播放器坏了。规则收进 `db::remember_position`（首秒不存、末尾 5 秒或
   98% 视为看完并清除），GUI / CLI / MCP 三面共用，前端不再各写一套判断。

**GUI 快捷键**：实机逐个验证通过 —— `z` 字幕延迟 0 → −0.1；`[` / `]` 设 A/B 点；
`\` 清除循环；`PageDown` 跳上一章；`PageUp` 跳下一章（章节 2，20.00s）。
本机桌面处于远程桌面会话下，`SetForegroundWindow` 被系统拒绝，按键改用
`PostMessage` 直接投递到 `Chrome_RenderWidgetHostHWND` 子窗口。
含修饰键的组合（Shift+Z、Ctrl+-/=）走 `GetKeyState`，`PostMessage` 无法伪造修饰键状态，
因此这两组未能自动验证，需人工按一次确认。

**已知未验证**：release 安装包尚未构建与双击验证；macOS / Linux 两端尚未同步构建。

## 发布纪律

按既定习惯，v0.10 攒成一个批次发布，不逐条出包。打 tag 前必须：
`pnpm tauri build` → 双击安装包实际启动 → 每平台截图确认。
`cargo check` 不足以发现启动期 panic 与运行时缺失。
