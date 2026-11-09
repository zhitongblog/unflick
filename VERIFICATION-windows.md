# Windows 实机验证记录（v1.0）

这份记录把 `ROADMAP.md` 中在 Windows 上挂账待验的 GUI 项（v0.11 #1 缩略图预览、v0.11 #3 鼠标手势与滚轮音量、v0.12 #4 网络共享路径指引、v0.13 播放条投屏面板、v0.14 双语字幕与首启引导）以及 v1.0 新增的四条主线（首屏居中列宽修复、跨媒体库字幕检索与结构化摘要、用户脚本系统、浏览器扩展 HTTP 桥与 `unflick://` 协议注册）一次跑完。

## 环境

```text
Windows 11 (x86_64) — AMD Radeon Graphics + NVIDIA GeForce RTX 3070 Laptop GPU + GameViewer Virtual Display Adapter
repo:      D:\code\unflick\unflick
binary:    src-tauri\target\debug\unflick.exe  —  cargo build --bin unflick --features custom-protocol
```

全程使用独立控制端口与临时目录，不碰宿主机的 `19542` 端口与真实用户数据：

```powershell
$env:UNFLICK_CONTROL_ADDR = "127.0.0.1:29891"
$env:UNFLICK_DATA_DIR     = "$env:TEMP\uf-win-verify\data"
$env:UNFLICK_CONFIG_DIR   = "$env:TEMP\uf-win-verify\config"
$env:UNFLICK_LOG          = "$env:TEMP\uf-win-verify\gui.log"
```

---

## 实机抓到的 4 个真问题（均已修复并入库）

在驱动真实 Windows 窗口的过程中抓到了 4 个编译与 headless 测试均无法发现的问题：

1. **混合显卡 + 虚拟显示器下 WGL `ChoosePixelFormat` 死锁**：
   在同时装有集显（AMD Radeon）、独显（NVIDIA RTX 3070）和虚拟显示器（GameViewer）的 Windows 11 机器上，`glutin` 调用 `opengl32.dll` 的 `ChoosePixelFormat` 枚举 ICD 驱动时会永久阻塞，导致 `bring_up_video_pipeline` 无法返回、窗口不显示、控制端口不监听。
   **修复**：与 Linux 端一致，Windows 视频表面保留原生 `WS_POPUP` 分层窗口（`set_geometry`、`set_exclusions` 镂空全屏控制栏、`set_visible`、`set_always_on_top`、`set_alpha`），并将渲染交由 mpv 原生 `--wid=<hwnd>`（Windows 下走 D3D11 交换链）+ `RenderLoop::start_passive`，完全绕开 `opengl32.dll` 的 WGL 像素格式枚举死锁。
2. **Windows 启动日志横幅顺序反了**：
   `main.rs` 的 `init_file_log` 在 Windows 分支上原本先向旧 `stderr` 写 `=== unflick <version> starting at <ts> ===` 横幅，随后才调 `SetStdHandle(STD_ERROR_HANDLE, h)` 重定向到日志文件，导致横幅丢失、`tests/gui_dev.rs` 的日志顺序断言失败。
   **修复**：将 `SetStdHandle` 挪到写横幅之前，与 Unix 的 `dup2` 顺序一致。
3. **WebView2 并发 IPC 导致滚轮多步调音乱序覆盖**：
   一次 `deltaY: 200` 滚轮事件拆成 5 步，同步循环发出 5 个 `invoke("player_set_volume")`（`95, 90, 85, 80, 75`）。Windows 上 WebView2 的 `http://ipc.localhost` 请求由多线程 worker 处理，第 4 个请求（`80`）偶发晚于第 5 个请求（`75`）落到 mpv，最终音量停在 `80`。
   **修复**：在 `playerStore.ts` 的 `setVolume` 中合并同一同步周期内的连续音量更新，只将最终目标值下发给后端。
4. **`ScriptSettings.tsx` 与 `LibraryPanel.tsx` 返回结构对齐**：
   `script_list` Tauri 命令返回 `{ dir, scripts: [...] }`，而 `ScriptSettings.tsx` 直接将其当作数组 `.map()`，导致打开设置面板时触发 React 渲染异常；`LibraryPanel.tsx` 的台词搜索结果键使用了 `entry_id` 而非后端 `LibraryTranscriptMatch` 的 `media_id`。两处均已修复并实机验证通过。

---

## 逐项验证结果

### 1. `gui_dev` 全链路窗口自测套件（`tests/gui_dev.rs`）

**结论：通过（27.56s）。**

覆盖 `dev_wait`（正向与 `--gone`）、`dev_eval`（正常求值、DOM 节点拒绝、异常栈提取）、`dev_snapshot`（树结构、无 `"undefined"` 标签、选择器 100% 自指）、`dev_text`、`dev_click`（含坐标命中进度条中点跳转与歧义拒绝）、`dev_capture`（内存与 `--output` PNG 写出，利用 `ICoreWebView2::CapturePreview`）、`UNFLICK_LOG` 启动打点、滚轮 5 步调音（`100 → 75`）、播放栏投屏面板状态一致性、以及 5 秒窗口几何风暴（`A_GEOMETRY_STORM`）下播放持续推进不崩溃。

```text
running 1 test
test the_bridge_drives_the_real_window ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 27.56s
```

### 2. 首启引导卡（v0.14）与首屏居中列宽（v1.0 Track 1）

**结论：通过。**

全新 `UNFLICK_CONFIG_DIR` 启动后，`[role="dialog"]` 在 7ms 内检出，包含中文引导文案、动态键帽（`Space` / `→` / `F`）与 CLI/MCP 安装命令；按 `Escape` 或点击「跳过」后 4ms 内关闭，`settings get --key onboarding_seen` 读回 `true`。

首屏居中容器补齐 `w-full max-w-md px-6` 后，实测宽度从旧版的 ~190px 恢复为标准 `448px`：

```json
{"width":448,"height":285,"className":"relative z-10 flex w-full max-w-md flex-col items-center gap-6 px-6"}
```

`dev capture --max-edge 320 --output idle.png` 产出 `1536×1004` PNG（49,395 字节）。

### 3. 进度条悬停缩略图预览（v0.11 #1）与鼠标手势（v0.11 #3）

**结论：通过。**

- **缩略图预览**：播放 `sample.mp4` 并暂停后，`unflick frame thumbnail 4.0` 返回 `{"success":true,"message":"preview at 4.0s → unflick-thumb.jpg"}`；在窗口进度条中点派发 `mousemove` 后，进度条浮层渲染出 JPEG data URL：
  ```json
  {"present":true,"srcPrefix":"data:image/jpeg;base64,/9j/4AA"}
  ```
- **滚轮音量与右键拖拽手势**：
  - 滚轮 `deltaY: 200` 准确从 `100` 降至 `75`（5 步 × 5%）。
  - 在视频区域按住右键从 `x=300` 拖到 `x=420`（`gesture_right`），播放位置从 `0.4s` 跳至 `5.4s`（前进 5 秒），且未弹出右键菜单。

### 4. 网络共享路径拒绝与挂载指引（v0.12 #4）

**结论：通过。**

```text
$ unflick play "smb://192.0.2.1/movies/sample.mkv"
{
  "success": false,
  "message": "cannot open smb://192.0.2.1/movies/sample.mkv: no smb:// support in this build. SMB URLs are not supported — map or open the share in Explorer, then play the UNC path (\\\\server\\share\\file.mkv)."
}

$ unflick play "nfs://192.0.2.1/export/sample.mkv"
{
  "success": false,
  "message": "cannot open nfs://192.0.2.1/export/sample.mkv: no nfs:// support in this build. NFS URLs are not supported — mount the export with Client for NFS, then play the mapped drive path."
}
```

### 5. 双语字幕（v0.14）

**结论：通过。**

加载主字幕 `sample.srt` 与副字幕 `sample.zh.srt` 后执行 `unflick subtitle bilingual --secondary 2`，Windows 打包的 libmpv 支持 `secondary-sub-pos`，采用 `stacked` 布局（主轨 `sub_pos: 100.0`，副轨 `secondary_sub_pos: 94.5`）：

```json
{
  "success": true,
  "message": "bilingual on: sample.zh.srt + sample.srt",
  "data": {
    "delay": 0.0,
    "enabled": true,
    "layout": "stacked",
    "secondary_delay": 0.0,
    "secondary_sub_pos": 94.5,
    "sub_pos": 100.0
  }
}
```

### 6. AI 深度理解：跨库台词检索与结构化视频摘要（v1.0 Track 2）

**结论：通过（CLI + MCP + GUI 三端对齐）。**

- **CLI / MCP 结构化摘要**：`unflick transcript summary --segments 3` 将当前视频字幕切分为 3 个时间段，返回 `cue_count: 3`、`word_count: 33`、`speaking_seconds: 8.5`、高频关键词 `keywords` 及每段起止时间与预览文本。
- **CLI / MCP 跨库检索**：`unflick library transcript "every subtitle"` 在已扫描媒体库中定位到 `sample.mp4` 的 `5.0s–8.0s` 字幕行。
- **GUI 媒体库「台词」模式**：按 `L` 打开媒体库面板，切换顶栏胶囊按钮至「台词」，输入 `"every subtitle"`，实时渲染出匹配卡片 `sample 0:05 Searching across every subtitle in the library works.`；点击该卡片后自动打开 `sample.mp4` 并跳转到 `5.0s` 处开始播放（实测 `status` 读回 `position: 5.66s, state: "playing"`）。

### 7. 插件 / 用户脚本系统（v1.0 Track 3）

**结论：通过（CLI + MCP + GUI 三端对齐）。**

- `unflick script add demo.lua` 将脚本安装至 `<config_dir>\scripts\demo.lua` 并立即热加载进运行中的 mpv 实例。
- `unflick script list / disable / enable / remove` 在 CLI 与 MCP 下完整管理启用状态（持久化于 `settings.json` 的 `disabled_scripts`）。
- 按 `Ctrl+,` 打开 GUI 设置面板，「用户脚本」分区显示 `demo.lua`、启用开关、移除按钮及说明文案 `"放在 scripts 目录里的 mpv .lua 与 .js 脚本会在播放时自动加载。"`。

### 8. 浏览器扩展 HTTP 控制桥与 `unflick://` 协议注册（v1.0 Track 4）

**结论：通过。**

- **Windows 注册表 URL 协议**：启动 GUI 后自动在 `HKCU\Software\Classes\unflick` 下完成注册，`shell\open\command` 指向 `"...\unflick.exe" "%1"`，支持 `unflick://play?url=...` 与 `unflick://queue?url=...`。
- **同端口 HTTP 桥**：在控制端口 `127.0.0.1:29891`（默认 `127.0.0.1:19542`）上与行分隔 JSON 共存：
  - `OPTIONS /open` 返回 `204 No Content`，携带 `Access-Control-Allow-Origin: *` 与 `Access-Control-Allow-Private-Network: true`（满足 Chromium PNA 规范）。
  - `GET /status` 直接返回当前播放器 JSON 状态。
  - `POST /open`（`{"url": "...", "action": "play" | "queue"}`）与 `POST /rpc`（通用命令转发）正常工作。

---

## 全量自动化测试汇总（Windows）

| 套件 | 命令 | 结果 |
|---|---|---|
| 前端单元测试（Vitest） | `pnpm test -- --run` | **9 个文件，126 passed**（含 8 语言 `locales.test.ts` 与 `hardcoded.test.ts`） |
| 前端类型检查与生产构建 | `pnpm build` | **0 errors**，产出 `dist/` |
| Rust 单元测试 | `cargo test --lib` | **192 passed, 0 failed** |
| 播放与控制面集成测试 | `cargo test --test playback -- --test-threads=2` | **113 passed, 0 failed** |
| AI 理解与脚本/跨库搜索集成测试 | `cargo test --test understanding` | **30 passed, 0 failed** |
| GUI 开发者桥集成测试 | `cargo test --test dev` | **15 passed, 0 failed** |
| DLNA 投屏集成测试 | `cargo test --test cast` | **15 passed, 0 failed** |
| 光盘与 ISO 集成测试 | `cargo test --test disc` | **9 passed, 0 failed** |
| 会话自动保存与恢复测试 | `cargo test --test session` | **6 passed, 0 failed** |
| 旧版残留清理集成测试 | `cargo test --test cleanup` | **5 passed, 0 failed** |
| 启动时间线集成测试 | `cargo test --test startup` | **3 passed, 0 failed** |
| 真实窗口全链路自测 | `cargo test --test gui_dev --features custom-protocol` | **1 passed（全阶段断言通过）** |
| `AGENTS.md` 同步校验 | `bash scripts/gen-agents.sh --check` | **通过** |
