# Linux 实机验证记录

这个文件的理由和 `VERIFICATION-macos.md` 一样：说清楚**哪些功能在 Linux 上真的
被驱动过**，哪些只是「编译过了、headless 全绿」。

在这次之前，**Linux 上一条 GUI 功能都没有被看过**。Windows 是几个月前手工验的，
macOS 是两天前第一次逐条扫的（抓到 6 个编译通过、headless 全绿的缺陷）。
Linux 这一栏从 v0.10 起就写着「三平台同等」，而它从来没有过任何证据。

（正在写：环境搭建完成后逐条填。）

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
