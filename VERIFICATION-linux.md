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
