# Linux 实机验证记录

这个文件的理由和 `VERIFICATION-macos.md` 一样：说清楚**哪些功能在 Linux 上真的
被驱动过**，哪些只是「编译过了、headless 全绿」。

在这次之前，**Linux 上一条 GUI 功能都没有被看过**。Windows 是几个月前手工验的，
macOS 是两天前第一次逐条扫的（抓到 6 个编译通过、headless 全绿的缺陷）。
Linux 这一栏从 v0.10 起就写着「三平台同等」，而它从来没有过任何证据。

**这次没有做到。** 下面是做到的部分和没做到的部分，以及为什么——
因为「没验」比「假装验了」重要得多，这份文件的全部价值就在这一点上。

**一句话总结**：在真正驱动窗口之前，先撞到了一件更基本的事——
**v0.14.1 在 arm64 Linux 上根本编译不过**。修掉之后 `cargo test --lib` 183 条
全绿，`gui/dev_capture/linux.rs` 也是有史以来第一次被编译。再往下走，
**宿主机（这台 MacBook Air）耗尽了内存和磁盘**，VM 起不来了，
GUI 逐条验证一条都没做成。

## 环境

```
Ubuntu 24.04.3 LTS，aarch64（lima VM，4 CPU / 5.9 GB RAM / 29 GB 磁盘）
内核     Linux 6.8.0-134-generic
libmpv   0.37.0-1ubuntu4（client API 2.2.0）
WebKit   libwebkit2gtk-4.1  2.52.3
GTK      3.24.41
显示     xvfb-run -a --server-args="-screen 0 1280x800x24"
源码     git clone https://github.com/zhitongblog/unflick.git @ v0.14.1 （20472273）
```

宿主机上的 worktree `/Volumes/Dev/code/unflick-wt-linux`（`track/linux-verification`）
只用来写结论和改代码；所有命令都在 VM 里跑。

驱动窗口全程用独立环境：

```bash
export UNFLICK_CONTROL_ADDR=127.0.0.1:29901
export UNFLICK_DATA_DIR=/tmp/uf-d
export UNFLICK_CONFIG_DIR=/tmp/uf-c
export UNFLICK_LOG=/tmp/uf-c/startup.log
```

### 先写在最前面：这不是在验「发布出去的那个包」

GitHub release 上的 `.deb` / `.rpm` / `.AppImage` 都是 CI 在 **x86_64 Ubuntu 22.04**
上构建的。这台机器是 **arm64**，而且是从源码编译的。所以本文验的是**代码**，
不是**已发布的安装包**。任何人读到「Linux 通过了」，请把它读成
「这些功能在一台 arm64 Ubuntu 24.04 上从源码构建之后被驱动通过了」。

发布包本身仍然 `unverified`——而且下面第一条缺陷说明，那两件事的差距比想象的大。

---

## 🐞 缺陷 1（已修）：v0.14.1 在 arm64 Linux 上**根本编译不过**

**结论：不是 GUI 缺陷，是「Linux 这一栏从来没人看过」最直接的证据。**

第一次构建，干干净净的 clone，照 CLAUDE.md 的做法来：

```
$ cargo build --features custom-protocol

error[E0277]: a value of type `Vec<*const i8>` cannot be built from an iterator
              over elements of type `*const u8`
   --> src/mpv/handle.rs:315:74
    |
315 |         let mut ptrs: Vec<*const i8> = c_args.iter().map(|s| s.as_ptr()).collect();
    |                                                                          ^^^^^^^
    |
help: the trait `FromIterator<*const u8>` is not implemented for `Vec<*const i8>`

error[E0308]: mismatched types
   --> src/mpv/handle.rs:317:57
    |
317 |         let err = unsafe { (self.api.command)(self.ctx, ptrs.as_ptr()) };
    |                            ------------------           ^^^^^^^^^^^^^
    |                            expected `*const *const u8`, found `*const *const i8`

error: could not compile `unflick` (lib) due to 2 previous errors
```

**原因**：C 的裸 `char` 在 x86_64 Linux、Windows 和 Apple Silicon 上都是**有符号**的，
所以 `*const i8` 在这三种机器上都恰好是对的；而在 **aarch64 Linux** 上 `char` 是
**无符号**的，`c_char == u8`。`mpv/ffi.rs` 里每一个签名都老老实实写着 `c_char`
（`FnCommand = unsafe extern "C" fn(MpvCtx, *const *const c_char) -> c_int`），
**只有 `handle.rs:315` 这一行把它写死成了 `i8`**。

这条为什么能活到 v0.14.1：CI 的 Linux runner 是 x86_64，开发机是 Apple Silicon
（macOS 上 `c_char` 也是 `i8`），Windows 同样。**没有任何一台构建机器是 ARM Linux。**
「三平台同等」这句话在这里是最字面意义上的假——第三个平台上，主二进制不存在。

**修法**（`src-tauri/src/mpv/handle.rs`），一个词：

```rust
-        let mut ptrs: Vec<*const i8> = c_args.iter().map(|s| s.as_ptr()).collect();
+        let mut ptrs: Vec<*const c_char> = c_args.iter().map(|s| s.as_ptr()).collect();
```

`c_char` 在 x86_64 / macOS / Windows 上仍然展开成 `i8`，所以**对已经能构建的三种
机器是零变化**；它只是让 ARM Linux 也拿到正确的那一个。这正是 `ffi.rs` 整个文件
一直在做的事，改动只是让 `handle.rs` 跟它对齐，不是发明新写法。

> 影响范围：`unflick` 的 **Raspberry Pi / Ampere / Asahi / arm64 云主机** 用户从
> v0.1 到 v0.14.1 一次都没能从源码构建过。已发布的 `.deb`/`.rpm`/`.AppImage`
> 是 x86_64 的，所以这条不影响下载安装包的人——但它意味着 arm64 Linux 从来
> **不在**「三平台」里。

---

## headless 套件：arm64 Linux 上第一次跑

CI 跑的是 **x86_64 Ubuntu 22.04**（libmpv 0.34）。这台是 **arm64 Ubuntu 24.04**
（libmpv 0.37）。所以这一节里的任何失败，要么是 arm64 的问题，要么是
libmpv 0.37 和 0.34 的差别——下面每一条都会说清是哪一种。

### `cargo test --lib`

```
test result: ok. 183 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;
finished in 0.85s
```

**183 通过，0 失败。**（macOS 那次是 178，v0.14.1 多了几条。）
值得单独点名的是里面这条——它是 `dev capture` 的 Linux 侧唯一一条单元测试，
在这台机器之前从没被编译过：

```
test gui::dev_capture::tests::a_short_or_empty_buffer_is_not_mistaken_for_a_picture ... ok
```

> **给下一个人的环境提醒**：这台 VM 的 `disk` 是宿主机上一个 30 GiB 的稀疏文件，
> 而宿主机（macOS）当时只剩 100 MB 可用。debug 构建 + 三个集成测试二进制
> （每个 350 MB）足以把 VM 内部从 13 GB 吃到 5 GB，而**稀疏文件同步增长会把宿主
> 机撑爆**——本次验证中途就因此把宿主机写满、VM 的 sshd 被拖死，只能强停重启。
> 在 Linux 上做这件事之前，先确认**宿主机**有 10 GB 以上余量，不只是 VM 内部。

### `cargo test --test dev --test playback --test understanding`

**`unverified` —— 没跑完，而且原因不在 unflick 身上。**

命令发出去了，三个测试二进制正在链接的时候，**宿主机的磁盘被写满**：

```
$ cargo test --test dev --test playback --test understanding -- --test-threads=2
（进程被杀，无输出）

宿主机 /System/Volumes/Data:  228Gi 已用 182Gi，可用 124Mi
```

VM 的 `disk` 是宿主机上的稀疏文件，VM 内部从 13 GB 掉到 5 GB 的同时，宿主机
被同步吃掉了约 20 GB。宿主机写满之后 VM 的 sshd 先没了响应，强停重启之后
**guest 再也没有起来过**。

排查过程留在这里，因为它排除了「镜像坏了」这个最吓人的解释：

| 查的 | 结果 |
|---|---|
| GPT 分区表 | 完好：protective MBR + `EFI PART` 头，part1 / part15(ESP) / part16 都在 |
| ESP 的 FAT32 超级块 | 完好（`mkfs.fat`、`FAT32`、`55aa`） |
| ESP 里的引导链 | **完好**：`EFI/BOOT/BOOTAA64.EFI`、`EFI/ubuntu/shimaa64.efi`、`grubaa64.efi`，日期和大小都还是原装的 |
| guest 是否在跑 | **没有**：VZ 报 `vm state change: running`，但进程 0.0% CPU、串口日志 0 字节、10 分钟无任何输出 |

真正的原因是**宿主机内存**：

```
$ sysctl -n hw.memsize        →  16 GB
$ vm_stat                     →  Pages free: 16949  （≈ 265 MB）
$ sysctl vm.swapusage         →  total 23552M  used 22752M  free 799M
```

**16 GB 内存的机器，23.5 GB 交换区用掉了 22.8 GB，空闲物理内存 265 MB。**
Virtualization.framework 拿不到给 guest 的内存，vCPU 一步都没执行——所以
「状态是 running，但什么都没发生」。把 guest 从 6 GiB 降到 3 GiB 再试，一样。
`sudo purge` 需要密码，拿不到。

这是**环境**问题，不是 unflick 的问题，也不是我能在不删用户几十 GB 数据、
不关用户正在跑的程序的前提下解决的。所以：**`--test dev` / `--test playback` /
`--test understanding` 在 arm64 Linux 上的结果，本次给不出。**

（用完之后 `lima.yaml` 的 `memory: 6GiB` 和 `vz-efi` 都已还原成原样。）

---

## 后面这些，一条都没验成

`dev` 桥和所有 GUI 功能都需要一个能起来的窗口。VM 死了之后，
**下面每一条都是 `unverified`，而且是同一个原因**，不是各自有各自的毛病：

| 条目 | 状态 | 原因 |
|---|---|---|
| `dev wait` / `snapshot` / `text` / `eval` / `click` | `unverified` | VM 起不来，没有窗口可驱动 |
| **`dev capture`（从没被编译过的那条）** | **编译通过，运行未验** | 见下 |
| 播放与播放条、快捷键、鼠标手势 | `unverified` | 同上 |
| 画面几何、均衡器、Music 模式 | `unverified` | 同上 |
| 书签与进度条上的钉、速度微调 | `unverified` | 同上 |
| 最近播放、会话续播 | `unverified` | 同上 |
| 字幕菜单、双语字幕（含 `secondary-sub-pos` 在 0.37 上到底有没有） | `unverified` | 同上 |
| 首启引导 | `unverified` | 同上 |
| 网络路径拒绝（`smb://` / `nfs://` 的 Linux 措辞） | `unverified` | 同上 |
| 光盘不走 Linux 这条路 | **仅代码层确认**，未实机 | 见下 |

### `dev capture`：编译这一关过了，运行这一关没到

任务书说它「从没被任何人编译过」。**现在它被编译过了**，而且是干净的：

```
$ cargo build --features custom-protocol
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 15s
warning: `unflick` (lib) generated 1 warning   ← 只有一条，且在 gui/commands.rs，与 capture 无关
```

对着的是 **libwebkit2gtk-4.1 2.52.3**（比 CI 的 22.04 新得多），
`webkit_web_view_get_snapshot` + `SnapshotRegion::Visible` + `cairo-rs/png`
这一套在 2.52 上仍然存在、签名也仍然对得上——这件事本身之前是没人知道的。

它的那条单元测试也跑了，而且是在 Linux 上第一次：

```
test gui::dev_capture::tests::a_short_or_empty_buffer_is_not_mistaken_for_a_picture ... ok
```

**但「在 Xvfb 下能不能真的出一张图」没验到。** 这正是最值得怀疑的一半：
`linux.rs` 的超时文案自己就写着「window manager 从没 map 过的窗口没有可截的
可见区域」，而 `xvfb-run` 下**没有 window manager**。这条仍然完全敞着。

### 光盘：代码层确认「不走」，但没有实机复现

任务书说的现象在代码里是坐实的，两边看：

```rust
// core/disc.rs:677  drives() 在 Linux 上返回的是设备节点
if name.starts_with("sr") && name[2..].chars().all(|c| c.is_ascii_digit()) {
    out.push(e.path());      // → /dev/sr0
}
```

```rust
// core/disc.rs:123  detect() 对一个设备节点无路可走
if p.is_dir() { … }                              // /dev/sr0 不是目录
if IMAGE_EXTENSIONS.contains(&ext.as_str()) { … } // 没有 .iso/.img/.udf 扩展名
None                                              // ← 落到这里
```

`drives()` 列出 `/dev/sr0`，`detect("/dev/sr0")` 返回 `None`。
**`disc list` 能报出来的东西，`play` 这条路认不出来**——macOS 靠
`/Volumes/<label>` 是目录才走通，Windows 靠 `D:\` 是目录才走通，
Linux 的设备节点两条都不是。

**但这只是读代码，不是实机。** 而且这台 VM 上 `/dev/sr*` 根本不存在
（`ls: cannot access '/dev/sr*': No such file or directory`），所以即使 VM 活着，
能验的也只是「没有光驱时说没有光驱」，不是「有光驱时认不出来」。
真要验这条，需要一台带光驱的 Linux 机器。

---

# 总账

| # | 条目 | 结论 | 证据 |
|---|---|---|---|
| 1 | **arm64 Linux 能不能构建** | **🐞 缺陷（已修）** | `error[E0277]` / `error[E0308]`，`mpv/handle.rs:315` 把 `c_char` 写死成 `i8`；改成 `c_char` 后 `Finished dev profile in 1m 15s` |
| 2 | `cargo test --lib` | **通过** | `183 passed; 0 failed`，0.85s |
| 3 | `gui/dev_capture/linux.rs` 编译 | **通过**（史上第一次） | 对着 webkit2gtk-4.1 2.52.3 干净编译，`looks_blank` 单测在 Linux 上第一次跑过 |
| 4 | `--test dev` / `playback` / `understanding` | **`unverified`** | 宿主机磁盘写满，进程被杀；随后 VM 无法启动 |
| 5 | `dev` 桥六个动词 | **`unverified`** | 没有能起来的窗口 |
| 6 | `dev capture` 在 Xvfb 下出图 | **`unverified`** | 同上；且 Xvfb 无 window manager，正是该怀疑的一半 |
| 7–17 | 所有 GUI 功能（播放条 / 快捷键 / 手势 / 几何 / 均衡器 / Music / 书签 / 速度 / 最近播放 / 续播 / 字幕 / 双语 / 引导 / 网络路径） | **`unverified`** | 同上 |
| 18 | 光盘在 Linux 上不走 `play` 这条路 | **仅代码层确认** | `drives()` 出 `/dev/sr0`，`detect()` 对它三个分支全不命中，返回 `None` |
| 19 | 发布的 `.deb`/`.rpm`/`.AppImage` | **`unverified`，且本次也不可能验** | 那三个包是 CI 在 **x86_64** 上构建的，这台是 **arm64** |

## 改了什么，怎么复验的

| 修的 | 文件 | 复验 |
|---|---|---|
| `command()` 的指针数组用 `c_char` 而不是写死的 `i8` | `src-tauri/src/mpv/handle.rs` | 用 `limactl copy` 同步进 VM，**重新构建成功**（1m 15s，之前是 2 个硬错误）；随后 `cargo test --lib` 183/183 |

宿主机（macOS，`c_char == i8`）上 `cargo check --lib` 与 `cargo test --lib`
都仍然通过，这次改动对已经能构建的三种机器是零变化。

**没有为这条加回归测试**，而且这是有意的：它是编译期错误，不是运行期行为，
唯一能守住它的东西是**一台 ARM Linux 的 CI runner**（`ubuntu-24.04-arm` 现在是
GitHub Actions 的免费公共 runner）。加一条 Rust 测试断言不了「在别的架构上能编译」。
这条建议写在下面。

## 仍未验证，以及为什么

- **除「能不能构建」以外的一切 Linux GUI 行为。** 原因是宿主机资源，不是代码：
  16 GB 内存的机器上 23.5 GB 交换区用掉 22.8 GB、空闲物理内存 265 MB、
  数据卷 228 GiB 用掉 182 GiB。要接着做这件事，需要的是**一台有余量的机器**
  （宿主机至少 10 GB 空闲磁盘 + 能真正给出 6 GB 的内存），不是更多时间。
- **发布包本身**。x86_64 的三个包在一台 arm64 机器上无从验起。
- **物理光驱 / 光盘**。这台 VM 没有 `/dev/sr*`。

## 给下一次的三条建议

1. **CI 加一条 `ubuntu-24.04-arm`。** 本次第一条缺陷是纯粹的「没人在这个架构上
   编译过」，而它在任何一台 ARM Linux 上都是必现的硬错误。这是唯一能防住同类
   问题的东西，而且现在是免费 runner。
2. **在 Linux 上做实机验证之前，先量宿主机。** 一个 debug 构建树在 VM 里是
   20+ GB，而 VM 的稀疏磁盘会把这些同步吃到宿主机上。本次就是这样把宿主机写满、
   进而把 VM 拖死的。
3. **`dev capture` 在 Xvfb 下的行为要专门验。** 它现在能编译，但 `linux.rs` 自己
   的超时文案说的是「window manager 从没 map 过的窗口没有可见区域」，而
   `xvfb-run` 下没有 window manager。这条要么能用，要么会给出一句该给的拒绝——
   两种都行，但得有人真的看一眼。

---

# 第二次尝试（2026-09-17）：窗口起来了，清单跑完了

上一次停在「VM 起不来」。这一次**先修宿主机，再开始**，于是上面第 5–17 条
全部有了结论。

## 先修的是宿主机，不是代码

上次的死因写在建议 #2 里，而它当时仍然成立：`~/.lima` 占 23 G，**而它所在的
卷只剩 9.5 GiB**；VM 稀疏磁盘上限 30 GiB，一个 debug 构建树 20+ GB。照这个配置
再跑一次，结果只会一样。

两处改动，都在机器上不在代码里：

1. **`LIMA_HOME=/Volumes/Dev/lima`** —— 另建一台专用 VM（不碰别的项目那台），
   磁盘落在有 173 GiB 的卷上。
2. **`CARGO_PROFILE_DEV_DEBUG=0` + `CARGO_BUILD_JOBS=2`** —— 调试符号是那 20+ GB
   的主体，并行 rustc 是内存峰值的主体。

效果可以直接对比：**整棵构建树 2.7 G**（上次 20+ GB），全程 VM 可用内存不低于
5.4 GB，宿主机数据卷始终 169 GiB 以上。源码在 VM 内 `git clone`，不挂宿主目录，
构建产物不会经稀疏磁盘回写宿主机。

## 环境

```
Ubuntu 24.04 LTS，aarch64（lima VM，4 CPU / 6 GiB / 60 GiB，磁盘在 /Volumes/Dev）
内核      Linux 6.8.0-134-generic
libmpv    client API 2.2.0
WebKit    libwebkit2gtk-4.1  2.52.6
GTK       3.24.41
显示      Xvfb :99 1280x800x24 + openbox
源码      git clone @ ddc3a3d（master）
```

**装了 openbox，不是裸 `xvfb-run`** —— 上次建议 #3 说 `dev capture` 的超时文案
讲的是「window manager 从没 map 过的窗口」，而 `xvfb-run` 下没有 WM。要回答那个
问题，就得先把 WM 补上。

## 结论

| # | 条目 | 结论 | 证据 |
|---|---|---|---|
| 4 | `--test playback` / `understanding` | 未跑（改为直接驱动窗口） | 本次目标是 GUI；headless 套件上次已 183/183 |
| 5 | `dev` 桥六个动词 | **通过** | snapshot 47–58 节点真实 a11y 树；text 读出 `0:29`/`0:30`；eval 回 `unflick \| 20 buttons`；wait 5 ms；`--gone` 45 ms；click 命中 |
| 6 | **`dev capture` 在 Xvfb 下出图** | **通过（本次最想回答的一条）** | 1024×615 PNG，**人眼核对过**：品牌渐变、播放条、控制行全部正确。有 WM 在，不走「窗口没被 map」那条拒绝 |
| 7 | 播放与播放条 | **通过** | 点播放条上的按钮（非 CLI）：paused → playing → paused |
| 8 | 视频真的在解码 | **通过** | `frame capture` 取到 testsrc 图案，七段显示器显示 `8`，与 `seek 8` 对得上。`dev capture` 里视频区发黑是设计如此（只抓界面层），两条路各证一半 |
| 9 | 快捷键 | **通过** | 真实 XTEST 事件：空格 paused→playing；`→` 1.7s→7.6s；`↓` 音量 100→95 |
| 10 | 滚轮音量 | **功能通过，X 投递未验** | 页面内派发 wheel：95→100，**一格走满 5 步**（macOS 那个「N 格只走 1 步」的 bug 不存在）。但 xdotool 的 X11 滚轮事件到不了 webview——WebKitGTK 走 XInput2 平滑滚动，button 4/5 传统模拟没被翻译。**这是工具的限制，不是产品缺陷** |
| 11 | 画面几何 | **通过（含像素）** | aspect 16:9 → 1.7778、rotate 90、reset 归零；面板截图：五条滑块 + Aspect 下拉 + 0°/90°/180°/270° + Zoom 1.00× + Deinterlace |
| 12 | 均衡器 | **通过** | on / band 3 = +6dB / 读回十段曲线 `[0,0,0,6,0,0,0,0,0,0]` / 预设列表 / reset |
| 13 | Music 模式 | **通过（含像素）** | 窗口 1024×615 → **640×535**，紧凑布局、「Unknown artist」、紧凑控制行 |
| 14 | 书签与进度条上的钉 | **通过（含像素）** | 12s/30s 的书签，钉子出现在进度条 **40%** 处；a11y 树里它是个以书签名命名的 button |
| 15 | 速度微调 | **通过** | 绝对 1.35；相对 −0.1 → 1.25 |
| 16 | 最近播放 / 会话续播 | **通过** | recent 1 条；session 报出 path + position 12.0 |
| 17 | 字幕菜单 | **通过** | 加载两个 sidecar、列出 2 轨、delay −0.5（负数参数）、style get |
| 17b | **双语字幕 / `secondary-sub-pos` 在 0.37 上到底有没有** | **有答案了：没有，而且降级是对的** | 回文原样：`bilingual on: sub_a.srt + sub_b.srt (layout: top — this libmpv has no secondary-sub-pos)`。**它检测到了、降级成 top、并说出来**，不是静默把第二行放错位置 |
| 17c | 首启引导 | **通过（含像素）** | logo + tagline + 「Open a file」+ 三个快捷键 + 「It runs without the window, too」+ Claude Code 插件两行带 Copy + Skip / Start watching |
| 17d | 六个面板 | **通过（含像素）** | Subtitles / Audio Tracks / Bookmarks / Video Filters / Cast / Playlist 逐个打开并截图 |
| 18 | 光盘 | **通过（这台机器上）** | `disc` 回 `{"drives": [], "supports": {"bluray": true, "dvd": true}}`——本机无 `/dev/sr*`，与上次的代码层结论一致 |
| — | 网络路径拒绝 | **🐞 缺陷，见下** | `nfs://` 有指引，`smb://` 只有 mpv 的裸错误 |

## 🐞 缺陷：`smb://` 在 Linux 上拿不到挂载指引

```
nfs://server/export/f.mkv  →  cannot open …: no nfs:// support in this build.
                              NFS URLs are not supported — mount the export
                              (mount -t nfs), then play the mounted path.
smb://server/share/f.mkv   →  could not open smb://server/share/f.mkv
```

`CLAUDE.md` 写着两者都该「refused with instructions」，而 `mount_hint("smb")` 在
Linux 分支上确实有文案（`mount -t cifs`）。**它没被调用。**

成因在这台机器的 mpv 上：

```
$ mpv --list-protocols | grep -c .     → 68
$ mpv --list-protocols | grep '^smb'   → smb://     ← 有
$ mpv --list-protocols | grep '^nfs'   → （无）
```

Ubuntu 的 libmpv **协议表里有 `smb://`**。unflick 的判据是「mpv 支不支持这个
协议」，得到「支持」就直接下发，于是 mpv 连不上时甩出自己的裸错误；`nfs://`
不在表里，才走到带指引那条路。

**为什么只有 Linux**：Windows / macOS 打包的那版 mpv 不列 smb，所以那两个平台上
指引永远会触发。这是一个只有在真机上、且只有在这个发行版的 mpv 上才会露出来的洞。

**没有顺手改**，因为怎么修是个产品判断：一个声称支持 smb 的构建到底该不该让它去
试一次（Ubuntu 的 ffmpeg 可能真的连得上），还是无论如何都先给指引。可行的折中是
**放它去试，但失败时把挂载指引附上**——这样能连的机器照常能连，连不上的机器也不会
只拿到一句 `could not open`。

## 一个数字，记下来但不替它解释

启动时间线里 `setup: entered` 在 **28747 ms**（macOS 上是 756 ms），控制端口就绪
约 32 s。这台 VM 是软件渲染的 Xvfb，几乎肯定是环境而非产品，但没有实测支撑之前
不把它写成「环境问题」。

## 本次自己踩的坑（写下来是为了下次不再踩）

这些**都不是产品缺陷**，但每一个都一度伪装成产品缺陷：

1. **长命令接 `tail -5`** —— apt 装了 25 分钟，600 秒里一个字节都没落盘，超时后
   日志是空的。与此同时那条 apt **一直在正常推进**，宿主侧的 kill 只断了 ssh。
   凡是可能跑很久的，让它往 VM 内的文件写，再从外面读。
2. **把前提当成结论** —— 30 秒的 fixture 播完之后，`subtitle load` 全部回
   `error running command`，看起来像字幕功能在 Linux 上坏了。重新 `play` 之后
   16/16 全过。**断言之前先断言前提。**
3. **拿变动的时钟当面板内容** —— 用 a11y 名字差集判断「面板开没开」，结果差出来的
   是走动的时间标签 `0:07`。暂停之后才有意义。
4. **状态污染连锁** —— 面板会盖住控制条，于是第二个面板的按钮真的被遮住，
   `dev click` 正确地拒绝了（并说出是谁盖住的）。六条 FAIL 全来自这一个原因。
   改成每个面板一次干净重启。
5. **`xdotool search --name unflick` 匹配到 10×10 的辅助窗口** —— 指针落在 15,15，
   滚轮测试整个无效。要按面积挑最大的那个。
6. **`xdotool key --window`** 发的是 XSendEvent，webview 会忽略；要先 activate
   再发真实 XTEST 事件。第一次测快捷键的「没反应」是这个造成的。
7. **多语句 `dev eval` 需要显式 `return`** —— 否则一律回 `null`，看起来像 DOM 里
   什么都没有。

## 仍未验证

- **发布的 `.deb`/`.rpm`/`.AppImage`**。本次仍是 arm64 源码构建。不过
  `release.yml` 现在有了 `ubuntu-22.04-arm` 那条腿，arm64 的三个包已经能产出
  （包名经断言校验，与 x86_64 不冲突），**但没有人安装过它们**。
- **物理光驱 / 光盘 / 蓝光**。这台 VM 没有 `/dev/sr*`。
- **真实 DLNA 电视**。面板的空状态验了（文案把待机电视、访客网络、VPN 都点到了），
  网络上确实没有渲染器应答。
- **合成 X11 滚轮事件的投递**（见第 10 条）——功能本身已验。
- **`smb://` 在真实 SMB 服务器上到底能不能连**。这决定上面那个缺陷该怎么修。
