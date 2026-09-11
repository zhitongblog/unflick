# macOS 实机验证记录

这个文件存在的理由，和 ROADMAP 里那张记账表一样：说清楚**哪些功能在 macOS 上
真的被人用眼睛（或者用 `dev snapshot`）看过**，哪些只是「编译过了、headless 测
试全绿」。

在这次之前，unflick 的每一条 GUI 功能验证都是在 Windows 上做的，靠 PrintWindow +
PostMessage 驱动窗口。macOS 一条都没验过。这次是 `unflick dev` 桥第一次真正被用起来。

## 环境

```
macOS 25.6.0 (Darwin)，Apple Silicon
worktree:  /Volumes/Dev/code/unflick-wt-verify   (track/macos-verification)
binary:    src-tauri/target/debug/unflick  —  cargo build --features custom-protocol
```

`custom-protocol` 不是可选的。第一次构建没带 `dist/`（还没跑 `pnpm build`），
`tauri-build` 的 proc macro 直接 panic：

```
error: proc macro panicked
error: could not compile `unflick` (lib) due to 1 previous error; 6 warnings emitted
```

先 `pnpm build` 再 `cargo build --features custom-protocol`，42 秒。

驱动窗口全程用独立环境，不碰用户真在看的那个播放器：

```bash
export UNFLICK_CONTROL_ADDR=127.0.0.1:29821
export UNFLICK_DATA_DIR=/tmp/uf-verify-d
export UNFLICK_CONFIG_DIR=/tmp/uf-verify-c
export UNFLICK_LOG=/tmp/uf-verify-c/startup.log
```

## 先记下来的一件事：窗口不在屏幕上时，看到的是「一切都不可见」

`unflick --allow-dev` 起来之后第一件事是找首启引导卡，结果：

```
$ unflick dev wait '[role="dialog"]' --timeout 5
{
  "success": false,
  "message": "gave up after 7180 ms — 1 match(es) for [role=\"dialog\"] are in the page but none is visible"
}
```

**在页面里，但不可见。** 追下去：

```
$ unflick dev eval 'JSON.stringify({hidden: document.hidden, vis: document.visibilityState, rAF: typeof requestAnimationFrame})'
{"hidden":true,"vis":"hidden","rAF":"function"}

$ unflick dev eval 'var d=document.querySelector("[role=\"dialog\"]"); var cs=getComputedStyle(d); …'
{"label":"为人类和 AI 打造的视频播放器","opacity":"0","visibility":"visible",
 "transform":"matrix(0.96, 0, 0, 0.96, 0, 0)","rect":[262.4,53.1,499.2,533.8],"parentOpacity":"0"}
```

`opacity: 0`、`scale: 0.96` —— 正是 Framer Motion 的 `initial`。窗口是隐藏的
（`document.hidden === true`），动画帧被挂起，**入场动画永远停在第一帧**。

dev_probe.js 的 `hiddenNote()` 已经把这件事写在**退场**动画上了（`wait --gone`
等不到）。但它对**入场**同样成立，而且后果更重：窗口没在前台的话，
`AnimatePresence` 挂载的每一个面板（字幕菜单、书签、均衡器……）都是 opacity 0，
`dev click` 会按「不可见」拒绝，整轮验证会变成一串假阴性。

屏幕**没有**锁（`ioreg -n Root -d1 -a` 里没有 `CGSSessionScreenIsLocked`）——
窗口只是没在前台。把它提到前台之后就正常了：

```bash
osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $PID) to true"
```

```
$ unflick dev eval 'JSON.stringify({hidden: document.hidden, vis: document.visibilityState})'
{"hidden":false,"vis":"visible"}
```

所以本次所有涉及面板 / 弹层的验证，都是在窗口置顶之后做的。

> `dev capture` 在窗口不可见时的拒绝是**对的**，不是故障：
> ```
> the window did not produce a snapshot within 10s. WebKit holds a snapshot back
> until the window next draws, and a window that is covered, on another Space, or
> minimised may never draw. Bring the unflick window to the front and retry.
> ```

---

## 桥本身：能用

```
$ unflick dev wait "body" --timeout 30
{"success": true, "message": "body appeared after 29 ms", …}

$ unflick dev eval "document.title"
{"success": true, "message": "unflick", "data": {"value": "unflick"}}

$ unflick dev snapshot --depth 8
{"success": true, "message": "38 node(s)", …}
```

snapshot 给出的选择器是能回喂给 `dev click` 的完整路径，例如
`body > div > div > div:nth-of-type(4) > div:nth-of-type(2) > div:nth-of-type(1) > button`
（名字 `媒体库 (L)`，role `button`）。界面是 zh-CN。

---

## 首启引导（v0.14，落地当天从没上过屏）

**结论：通过。**

### 卡片确实只在该出现的时候出现

全新 config 目录 + 不带文件启动，卡片在：

```
$ unflick dev text '[role="dialog"]'
"text": "unflick\n为人类和 AI 打造的视频播放器\n几乎什么都能打开——本地文件、已挂载的网络共享、串流链接。解码交给 mpv，编解码器不再是你的问题。\n打开文件\n……或把文件拖到窗口任意位置\n先记住这三个键\n\nSpace\n播放 / 暂停\n→\n快进\nF\n全屏\n没有窗口时它照样能跑\nunflick 自带命令行工具和 MCP 服务器。这个窗口能做的事，脚本或 AI 助手同样能做。\nunflick play <file>\n在 Claude Code 里，两行就够：\n/plugin marketplace add zhitongblog/unflick\n/plugin install unflick\n复制\n跳过\n开始观看"
"visible": true
```

`dev capture` 出的图（2048×1280）确认了排版：logo + 标题、三个键帽
（Space / → / F，走的是 `formatKey`，不是硬编码）、CLI/MCP 那一块、右下角
「跳过」「开始观看」。**没有一个节点叫 "undefined"。**

### 四条退出路径，条条都落盘

每条都验了两件事：卡片消失，且 `onboarding_seen` 从 CLI 读出来是 `true`
——窗口和 CLI 两个真相来源对上。

| 路径 | 命令 | 卡片 | `settings get --key onboarding_seen` |
|---|---|---|---|
| Esc | `dev eval 'document.dispatchEvent(new KeyboardEvent("keydown",{key:"Escape",bubbles:true}))'` | `left the page after 2 ms` | `true` |
| 跳过 | `dev click '[role="dialog"] button' --index 2` → `clicked button.rounded-lg.px-3 "跳过"` | `left the page after 2 ms` | `true` |
| 开始观看 | `dev click '[role="dialog"] button' --index 3` → `clicked button.rounded-lg.border "开始观看"` | `left the page after 5 ms` | `true` |
| 点遮罩 | 在 overlay 的 `(left+8, top+8)` 派发 mousedown/mouseup/click | `left the page after 2 ms` | `true` |

Esc 那条之后 settings.json 的全部内容就是：

```json
{
  "onboarding_seen": true
}
```

### 重新武装、以及不再回来

```
$ unflick settings set onboarding_seen false
{"success": true, "message": "set onboarding_seen", "data": {"key": "onboarding_seen", "value": false}}
```

配合 `location.reload()`，卡片每次都回来（`rearmed: True`，四轮里用了三轮）。
不重新武装就重启，卡片不回来：

```
### restart with onboarding_seen=true, no file
launched pid=31869 visible=true
$ unflick dev eval 'JSON.stringify({dialogs: document.querySelectorAll("[role=\"dialog\"]").length})'
{"dialogs":0}
```

### 一个值得单独记的正确行为：带着文件启动，标志不被消费

`onboarding_seen` 置回 false，然后带着 selftest.mp4 启动：

```
### re-arm, restart WITH a file — card must stay away
launched pid=34308 visible=true
{"dialogs":0}
  state: playing file: atchpad/selftest.mp4 dur: 20.0
  onboarding_seen still: False
```

卡片没出（`shouldShowOnboarding` 里 `playerState !== "stopped"` 那条），**而且
标志没有被顺手写成 true**。也就是说：从 Finder 双击一个片子第一次用 unflick 的
人，下次空手打开播放器时，引导还在等他。这条如果写反了，是那种没人会发现的 bug。

---

## 双语字幕（v0.14，落地当天从没上过屏）

**结论：通过，而且是这次验证里证据最硬的一条**（真窗口合成后的像素）。

### 先撞上一件不是 bug 的事

第一次试的时候，我用 CLI 加载了两个字幕文件，然后打开 GUI 的字幕菜单：

```
$ unflick subtitle list
  id=1 orig.srt  selected=false
  id=2 zh.srt    selected=true

$ unflick dev text '.glass-elevated'
字幕
没有加载字幕轨。          ← 菜单说一条都没有
延迟 − +0.00s +
加载字幕文件… / 在线查找字幕…
```

看着像经典的「列表没走到打开的面板里」。但不是：`playerStore` 的轮询只在
**文件变了**的时候重拉字幕轨（`if (s.file !== previousFile) … refreshSubtitles()`），
同一个文件上从 CLI 加载的轨道不会推给窗口。这是有意的，代码里写着。所以这条不
记为缺陷——但它确实意味着：**在同一个文件上用 CLI/MCP 加字幕，窗口菜单不会更新，
要切一次文件。** 记在这里，因为下一个从外面驱动窗口的人一定会再撞一次。

按它设计的路走（sidecar 放在视频旁边，换文件触发轮询）之后，菜单是对的：

```
$ unflick subtitle list
  id=1 title=en.srt selected=False secondary=False
  id=2 title=zh.srt selected=True  secondary=False

$ unflick dev text '.glass-elevated'
字幕
关闭
en.srt
zh.srt
双语字幕
延迟 − +0.00s +
加载字幕文件… / 在线查找字幕…
```

两条轨道、标题一致、**没有一个是 "undefined"** —— Windows 那次音轨菜单全是
"undefined" 的同类 bug，在 macOS 的字幕菜单上不存在。

### 开关真的驱动了后端

```
$ unflick dev snapshot --selector '[role="switch"]'
{"name": "同时显示原文和译文", "role": "switch", "state": {"checked": false}}

$ unflick subtitle bilingual
"message": "bilingual off"
"primary": {"id": 2, "label": "bili.zh.srt", "lang": "zh"}, "secondary": null

$ unflick dev click '[role="switch"]'
clicked button.flex.w-full "同时显示原文和译文"

$ unflick dev snapshot --selector '[role="switch"]'
{"name": "同时显示原文和译文", "role": "switch", "state": {"checked": true, "focused": true}}

$ unflick subtitle bilingual
"message": "bilingual on: bili.en.srt + bili.zh.srt"
"primary":   {"id": 1, "label": "bili.en.srt", "lang": "en"}
"secondary": {"id": 2, "label": "bili.zh.srt", "lang": "zh"}
"layout": "stacked",  "sub_pos": 100.0,  "secondary_sub_pos": 94.5
```

窗口的 `aria-checked` 和 CLI 读出来的 `enabled` 两边对上，主轨/副轨也对上。

### 两行真的叠在屏幕上 —— 这条只能用真像素证

`unflick screenshot` 证不了。它要的是干净的画面层：

```rust
// core/player.rs:475
self.mpv.command(&["screenshot-to-file", path, "video"])
```

`"video"` 就是「不含字幕和 OSD」，所以截出来的 20 秒测试片是彩条，一个字都没有。
`dev capture` 也证不了——它拿的是 WKWebView 那一层，而 mpv 在 macOS 上画在自己的
子窗口里。**两个内置工具都到不了这个问题**。

屏幕当时没锁、窗口在前台，所以用 `screencapture` 抓了真实合成后的窗口，裁出字幕带：

![两行字幕](docs/verification-macos/bilingual-two-lines.png)

上面一行 `原文那一行`（zh，secondary，`secondary_sub_pos` 94.5），下面一行
`The original line`（en，primary，`sub_pos` 100）。**不重叠，确实是两条带。**
bilingual.rs 里那句「mpv 不会自己叠，`secondary-sub-pos` 等于 `sub-pos` 时会塌成
一条」——叠加的算术在 macOS 上是对的。

菜单本身也一起进了这张图：

![字幕菜单](docs/verification-macos/bilingual-menu.png)

`en.srt` 前面是对勾（选中的那条），`zh.srt` 右边是 `第二行` 角标（副轨用角标而不是
对勾，因为它不是「选中」的意思），`双语字幕` 开关是开的。

### 落盘，以及换文件自动重新武装

```json
// /tmp/uf-verify-c/settings.json
{
  "onboarding_seen": false,
  "subtitle_bilingual": { "enabled": true, "layout": "stacked" }
}
```

换一个带自己 sidecar 的新文件（second.mp4 + second.en.srt + second.zh.srt）：

```
$ unflick play /tmp/uf-verify-media/second.mp4
$ unflick subtitle bilingual
  bilingual on: second.en.srt + second.zh.srt
  enabled True primary second.en.srt secondary second.zh.srt sub_pos 100.0 sec_pos 94.5
```

`after_play_hooks` 那条路在 macOS 上通的。而且**菜单自己跟上了**——文件换掉之后
菜单还开着，没有人碰它，开关读出来已经是 checked：

```
$ unflick dev snapshot --selector '[role="switch"]'
  switch: 同时显示原文和译文 checked= True
```

这正是 `refreshSubtitles` 里 `bilingual: mapped.some(t => t.secondary)` 那行在干的事。

从 GUI 关掉，一路通到底：

```
$ unflick dev click '[role="switch"]'   → clicked button.flex.w-full "同时显示原文和译文"
$ unflick subtitle bilingual            → bilingual off
$ cat settings.json                     → {"enabled": false, "layout": "stacked"}
```
