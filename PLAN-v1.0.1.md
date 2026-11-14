# unflick v1.0.1 — 深度理解与体验闭环

> **起点**：v1.0.0（2026-11-12）。三平台同等实机验证完成，插件系统、浏览器扩展、跨库台词检索与视频摘要已落地。
> **目标**：把路线图中最后一项「说话人分离」与多模态视觉理解补齐，清零三平台实机验证中挂账的细节缺陷，并打磨字幕交互与扩展生态。
> **核心铁律**：每项能力严格遵循 **core → daemon → CLI → MCP → 自动化测试 → GUI** 五面对齐。

---

## Track 1 — 实机挂账缺陷清零（Polish & Backlog Zeroing）

把 `VERIFICATION-macos.md` 与 `ROADMAP.md` 中明确记录但挂账未改的 4 个问题彻底收口：

| # | 问题与根因 | 修复方案 | 验证方式 |
|---|---|---|---|
| 1.1 | **慢加载 / 死管道误记「最近播放」**：`daemon.rs` 的 `play` 分支在判断 `outcome == LoadOutcome::Loaded` 之前就调了 `db.record_play(&src)`，导致 `loaded: false`（如断开的共享盘或 FIFO 阻塞）留在首屏最近播放里 | 仅当 `outcome == LoadOutcome::Loaded` 时立即写入 `record_play`；若返回 `StillLoading`，由后台轮询/状态线程在首次观测到 `FILE_LOADED`（duration > 0 且文件一致）时补记历史 | 集成测试：用阻塞 FIFO / 慢源验证 `loaded: false` 时 `recent list` 不入库，加载完成后入库 |
| 1.2 | **URL 对话框错误提示双语同屏**：输入 `smb://` 等不支持的协议时，`OpenErrorCard` 弹出本地化指引卡，同时 `UrlDialog` 内仍保留英文原串 | 当错误已被 `OpenErrorCard` 接管展示时，关闭或抑制 `UrlDialog` 内的重复原始错误行，确保同屏只有一份本地化错误说明 | `gui_dev` / `dev snapshot` 验证输入 `smb://` 时无双语重复报错 |
| 1.3 | **快捷键与鼠标绑定改后热生效**：目前 `keybindings` / `mouse_bindings` 仅在 GUI 启动时读取一次，通过 CLI/MCP 或设置面板修改后需重启才生效 | `core::events::EventKind` 新增 `Keybindings` / `Settings` 变更通知；CLI/MCP `keybind` / `mouse` / `settings` 修改时触发 `EventSink`，前端监听 `unflick:changed` 实时重拉绑定表 | 集成/窗口测试：运行中执行 `unflick keybind set` 后无需重启即可触发新键位 |
| 1.4 | **快捷键冲突提示国际化**：设置面板绑定快捷键冲突时，当前直接展示后端拼出的英文句子 | 冲突提示结构化返回冲突的 `action` 名，前端通过 8 语言 `i18n` 字典渲染本地化文案 | 前端 `locales.test.ts` + `hardcoded.test.ts` 覆盖 |

---

## Track 2 — 轻量级说话人分离（Speaker Diarization）

零额外重型运行时依赖，把「谁在什么时候说了什么」变成 `transcript` 的一等属性：

| 步骤 | 内容 | CLI / MCP / GUI 表面 |
|---|---|---|
| 2.1 | **字幕自带说话人解析（`core/transcript.rs`）**：识别 SRT/VTT/ASS 中常见的说话人标记（如 `[Speaker 1]:`、`ALICE:`、`- Bob:`、ASS 角色列 `Name` 字段、以及 `whisper.cpp --tinydiarize` 输出的 `[SPEAKER_TURN]` 标记），在 `Cue` 上新增 `speaker: Option<String>` | `unflick transcript get` / MCP `transcript_get` 自动携带 `speaker` 字段 |
| 2.2 | **轻量级声学 + 停顿启发式分离（`core/diarize.rs`）**：对无说话人标签的轨道，结合句间停顿阈值、标点对话交替模式，以及基于 `ffmpeg` 抽取 16kHz PCM 片段的短时能量/基频（F0 音高过零率与频谱质心）K-means 聚类，自动标注 `Speaker 1..N`；同时在 `whisper.rs` 支持可选 `--tinydiarize`（`-tdrz`）参数 | CLI: `unflick transcript speakers [--detect] [--count <n>]`<br>CLI: `unflick transcript rename-speaker <from> <to>`<br>MCP: `transcript_speakers` / `transcript_rename_speaker` |
| 2.3 | **按说话人检索与过滤**：扩展 `transcript search` 与 `transcript summary`，支持 `--speaker <name>` 过滤，并在 `TranscriptSummary` 中输出每位说话人的发言时长占比与台词数 | CLI: `unflick transcript search <q> [--speaker <name>]`<br>MCP: `search_transcript` 新增 `speaker` 参数 |
| 2.4 | **GUI 说话人标签与筛选**：在字幕面板/台词视图中以彩色徽章展示说话人，支持点击说话人徽章过滤只看该角色台词、或一键重命名角色 | GUI 字幕侧边栏 / 媒体库台词视图 |

---

## Track 3 — 视觉 / 多模态 AI 增强（Contact Sheet & Scene Detection）

单帧 `describe_frame` 只能看一个时间点；多模态模型回答「这段视频发生了什么、有哪些镜头切换」需要时间跨度上的视觉概览：

| 步骤 | 内容 | CLI / MCP / GUI 表面 |
|---|---|---|
| 3.1 | **多帧网格接触表（Contact Sheet，`core/vision.rs`）**：在指定时间区间 `[start, end]` 内均匀采样 `N` 帧（默认 `4×3 = 12` 帧），用 `ffmpeg` `select` + `scale` + `drawtext`（烧录时间戳）+ `tile` 滤镜合成为单张高信息密度 JPEG。一次 MCP 调用即可让视觉模型看清整段视频的画面演进，节省 90% 图像 token | CLI: `unflick frame sheet [--cols 4] [--rows 3] [--start <s>] [--end <s>] [--output <path>]`<br>MCP: `frame_contact_sheet`（返回单张拼图 image block + 各格时间戳数组） |
| 3.2 | **镜头 / 场景切换检测（Scene Detection，`core/scene.rs`）**：利用 `ffmpeg` `select='gt(scene,threshold)',showinfo` 扫描视频流，提取画面突变时间戳与场景得分；支持一键将检测到的场景边界转化为播放器虚拟章节（`Player::virtual_chapters`） | CLI: `unflick scene detect [--threshold <0.05..0.9>] [--to-chapters]`<br>MCP: `detect_scenes`（支持 `apply_as_chapters: bool`） |
| 3.3 | **GUI 章节メニュー联动**：在章节菜单（`ChapterMenu.tsx`）新增「从台词生成章节」与「按画面切换生成章节」快捷入口，生成后立即反映在进度条刻度上 | `ChapterMenu.tsx` + 进度条刻度实时刷新 |

---

## Track 4 — 字幕体验进阶（ASS/SSA 完整解析 & 交互式台词侧边栏）

| 步骤 | 内容 | CLI / MCP / GUI 表面 |
|---|---|---|
| 4.1 | **ASS / SSA 字幕解析（`core/transcript.rs`）**：`TEXT_SUB_EXTENSIONS` 纳入 `.ass` / `.ssa`；解析 `[Events]` 下的 `Format:` 与 `Dialogue:` 行，准确提取 `Start`、`End`、`Style`、`Name`（直接映射为 `Cue.speaker`）与正文（剥离 `{\pos(...)}`、`{\c&H...&}` 等绘图与特效控制码，保留换行 `\N`） | 所有 `transcript` 与 `library transcript` 命令原生支持 `.ass` / `.ssa` 外挂与内嵌轨 |
| 4.2 | **交互式逐句字幕侧边栏（`TranscriptPanel.tsx`）**：播放界面新增可呼出的「台词 / 转录」侧边抽屉（快捷键 `T`）：<br>- 随播放进度自动高亮并平滑滚动到当前台词<br>- 点击任意台词行立即跳转（`seek`）到对应毫秒<br>- 顶部内置实时搜索框、说话人筛选胶囊、以及「导出文本 / 摘要」按钮 | CLI: `unflick transcript export <path> [--format txt\|srt\|json]`<br>MCP: `transcript_export`<br>GUI: `TranscriptPanel.tsx` + 播放栏按钮 / 快捷键 `T` |

---

## Track 5 — 流媒体接力与播放列表生态（Browser Handoff & M3U/PLS）

| 步骤 | 内容 | CLI / MCP / GUI 表面 |
|---|---|---|
| 5.1 | **播放列表导入与导出（`core/playlist.rs`）**：支持标准 `.m3u` / `.m3u8`（非 HLS 索引时的本地播放列表）与 `.pls` 格式读写；相对路径按播放列表文件所在目录解析 | CLI: `unflick playlist save <path>` / `unflick playlist load <path> [--append]`<br>MCP: `playlist_save` / `playlist_load`<br>GUI: 播放列表面板新增「导入 / 导出」按钮 |
| 5.2 | **浏览器扩展时间戳接力与快捷键（`browser-extension/`）**：<br>- `unflick://play?url=...&t=<seconds>` 与 HTTP 桥 `POST /open` 支持携带 `seek` 秒数<br>- 扩展注入轻量脚本读取当前页面 `<video>` 元素的 `currentTime`（自动暂停网页原视频），实现 B 站 / YouTube 「从当前看到的秒数无缝接力到 unflick」<br>- `manifest.json` 声明 `commands` 快捷键（如 `Alt+Shift+U` 一键将当前页投递到 unflick） | `core::source::parse_unflick_uri` 扩展 `t` / `seek` 参数；`daemon.rs` `/open` 路由支持 `seek` 字段；更新 `browser-extension/` |

---

## 推进顺序与里程碑验收

1. **Phase 1（Track 1）**：实机挂账缺陷清零 → 先把既有地基擦干净，补充回归测试。
2. **Phase 2（Track 4.1 + Track 2）**：ASS/SSA 解析 + 轻量级说话人分离 → 打通 `Cue.speaker` 数据结构与 CLI/MCP 工具。
3. **Phase 3（Track 4.2）**：交互式台词侧边栏 GUI + 导出能力 → 将转录、搜索、说话人筛选在 GUI 中汇聚成完整体验。
4. **Phase 4（Track 3）**：多帧接触表（Contact Sheet）+ 场景切换检测（Scene Detection） → 扩充多模态 MCP 护城河。
5. **Phase 5（Track 5）**：M3U/PLS 播放列表导入导出 + 浏览器扩展时间戳接力 → 完善外部生态连接，最后跑全量单元、集成与 `gui_dev` 真窗验证。
