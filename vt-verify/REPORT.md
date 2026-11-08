# libghostty-vt Linux 构建与接口验证报告

- 日期：2026-02-09（按系统时间）
- 目标：在 Devuan 5（无 systemd）环境完成 Uzaaft/libghostty-rs 的 libghostty-vt 构建与 V1~V8 接口验证
- 结论分级：已验证事实 = 本次环境实际运行得到；工程判断 = 基于事实的推断，无直接测量；待验证假设 = 尚未证实
- 复现方式：`sh /root/Desktop/tty/vt-verify/checks/run_all.sh`（本次运行结果：10 项检查通过、0 项失败）

## 环境与版本（已验证事实）

| 项 | 值 | 来源 |
|---|---|---|
| 系统 | Devuan 5，无 systemd | `cat /etc/os-release` |
| Rust | 1.99.0 | `rustc --version` |
| Zig | 0.17.1，装于 `vt-verify/toolchain/zig` | ziglang.org/download/index.json，sha256 `3581863e…b17a` 校验通过 |
| libghostty-rs | commit `84373f8d48b8f0b794e259f74a71a52686616f5d`（v0.2.1，MSRV 1.96） | `vendor/PINS.md`、cargo metadata |
| ghostty 源码 | commit `0b8b9984ee560456194262d190aa850224edb1f4`（1.4.1-dev，minimum_zig_version 0.17.0） | `vendor/ghostty-src` build.rs `GHOSTTY_COMMIT` |

依赖处理：codeberg.org 整体 503 致 translate_c→aro 拉取失败，用 `zig fetch git+https://github.com/Vexu/arocc#d0c8c4d9c55daa7ef6e40cf0f630a5b5e900989b` 预取同 commit（hash `aro-0.0.0-JSD1QtuBNwCASyBtNF3pqTl_W3oAJQGEVyFAtrBSE_Pa` 一致），未改动上游代码。

## 逐项结果

### V1 构建产物（PASS）
已验证事实：`cargo build --workspace` 通过；产物 `libghostty-vt.a`（当前 85,849,368 字节，多次构建间在 85,841,712~85,852,080 间波动，见未解决问题 1）+ `libghostty-vt.so.0.1.0`（当前 91,926,785 字节）+ 35 个头文件，位于 `vendor/libghostty-rs/target/debug/build/libghostty-vt-sys-08ecac7d8b3af77c/out/ghostty-install/`。证据脚本 `checks/v1_artifacts.sh`，头文件清单落盘 `checks/v1_headers.txt`。失败原因：无。

### V2 可重复构建与离线（PASS）
已验证事实：`PINS.md` 落盘；`cargo clean` 后重建成功（56.60s）；在将网络请求指向本地不可达地址的条件下验证两条离线路径，均成功——zig 层 `--system` 与 cargo 层 `GHOSTTY_SOURCE_DIR`+`GHOSTTY_ZIG_SYSTEM_DIR`+`--offline`（2m51s）。`vt-verify/zig-system/` 按 nix 布局构造（24 个 `<hash>/` 解压目录，依据 ghostty 官方 `nix/build.zig.zon.nix`）。失败原因：无。

### V3 下游工程链接（PASS）
已验证事实：`checks/v3_project`（path 依赖）构建链接成功，运行输出 `downstream link ok`；`ldd` 仅 libc/libgcc_s/libpthread/libdl/ld-linux + libgtk3-nocsd，无 Zig 依赖（libgtk3-nocsd 由运行环境自动加载）；清单存 `vt-verify/v3_ldd.txt`。失败原因：无。

### V4 网格断言 a-f（PASS，6/6）
已验证事实（`tests/v4_grid.rs`）：
a) `"hello"` → 首行前 5 格字符 h/e/l/l/o；
b) `"\x1b[31mred\x1b[0m"` → "red" 三格前景红（ghostty 默认 palette red=(204,102,102)，断言按红主导口径 r>150 且 r>g+50 且 r>b+50），其后格恢复默认；
c) `"\x1b[2J\x1b[H"` → 光标 (0,0) 且网格全空；
d) 24 行终端输出 60 行带编号文本 → `scrollback_rows()>=36` 且视口最后非空行含 "line 60"；
e) `"你好abc"` → "你"/"好" 各占 2 格（第 2/4 格 `CellWide::SpacerTail` 续格标记），a/b/c 各 1 格；
f) `"\x1b[?1049h"` → `active_screen()==Screen::Alternate` 且 `scrollback_rows()==0`（alt 屏无历史）；`"\x1b[?1049l"` → 回 Main 且主屏内容恢复。
两处断言口径修正（已验证事实）：红色非纯 (255,0,0) 而是默认 palette (204,102,102)；末行 `\r\n` 使光标落在空行，故"视口底部为最后一行"按最后非空行断言。失败原因：无（首跑 2 项为断言口径问题，修正后全过）。

### V5 接口清单 11 项（PASS，均为"有"）
已验证事实：`checks/v5_api.sh` 逐项 grep 绑定源码确认存在，输出表见下。上游 C 头文件（`vendor/ghostty-src/include/ghostty/vt/`，共 216 个 `GHOSTTY_API` 函数）中抽查 6 个代表性 API，绑定均已封装，未发现"上游有、绑定无"的项（抽查范围见未解决问题）。失败原因：无。

| # | 能力 | 绑定 API | 位置（相对 `src/`） | 上游 C API |
|---|---|---|---|---|
| 1 | 遍历视口行与单元格 | `RenderState::update` + `RowIterator::new/update/next` + `CellIterator::new/update/next` | render.rs | `ghostty_render_state_row_iterator_new/next`、`ghostty_render_state_row_cells_new/next`（render.h） |
| 2 | 单元格字符与宽度/续格标记 | `CellIterator::graphemes/graphemes_len` + `raw_cell→Cell::wide`（`CellWide::{Narrow,Wide,SpacerTail,SpacerHead}`） | render.rs / screen.rs | `ghostty_grid_ref_graphemes`（grid_ref.h）、`ghostty_cell_get`（screen.h） |
| 3 | 前景/背景色与样式属性 | `CellIterator::fg_color/bg_color/style` + `Style{bold,italic,faint,blink,inverse,invisible,strikethrough,overline,underline}` | render.rs / style.rs | `ghostty_render_state_row_cells_get`（render.h）、`ghostty_style_default`（style.h） |
| 4 | 光标位置/可见性/样式 | `Terminal::cursor_x/cursor_y` + `RenderState::cursor_visible/cursor_blinking/cursor_visual_style` | terminal.rs / render.rs | `ghostty_terminal_get`（terminal.h）、`ghostty_render_state_get`（render.h） |
| 5 | 脏行/脏标记 | `RenderState::dirty` + `RowIterator` 脏行迭代（`next_dirty`） | render.rs | `ghostty_render_state_row_iterator_next_dirty`（render.h） |
| 6 | 滚动偏移与总行数 | `RenderState::viewport_y/viewport_col/viewport_row` + `Terminal::scrollback_rows` | render.rs / terminal.rs | `ghostty_terminal_get`（terminal.h） |
| 7 | 滚动操作（按行/顶/底） | `Terminal::scroll_viewport(ScrollViewport::{Top,Bottom,Delta,Row,DeltaPrompt})` | terminal.rs | `ghostty_terminal_scroll_viewport`（terminal.h:3585） |
| 8 | resize | `Terminal::resize(cols,rows,cw,ch)` | terminal.rs | `ghostty_terminal_resize`（terminal.h:3289） |
| 9 | alternate screen 状态 | `Terminal::active_screen` + `Screen::{Primary,Alternate}` | terminal.rs / screen.rs | `ghostty_terminal_get`（terminal.h） |
| 10 | 选区/可复制文本提取 | `Terminal::selection/select_all/select_line/select_word` + `Selection::format_selection_buf/alloc` | selection.rs | `ghostty_terminal_select_word/select_line`（selection.h:771） |
| 11 | 终端模式标志 | `Terminal::mode/set_mode` + `Mode::{X10_MOUSE,BRACKETED_PASTE,DECCKM,ALT_SCREEN_SAVE,...}` | terminal.rs | `ghostty_terminal_get/set MODE` + modes.h 常量 |

### V6 PTY 交互（PASS）
已验证事实：`tests/v6_pty.rs` 经 portable-pty 起 `/bin/sh`，后台线程读 PTY 经 channel 喂 `Terminal`（主线程独占 Terminal），`echo 你好` 入网格断言通过，resize 80x24→120x40 断言通过。失败原因：无（首版 `exit\n`+`wait` 无法正常退出，改为终止子进程并释放 PTY 主端解决，实现见 `tests/v6_pty.rs`）。

### V7 KeyEncoder 按键序列（PASS，2/2）
已验证事实（`tests/v7_key.rs`，先探针实测再断言）：普通模式 ArrowUp/Down/Left/Right → `\e[A/B/D/C`，Enter → `\r`，Backspace → `\x7f`，Tab → `\t`，Ctrl-C → `\x03`，Ctrl-D → `\x04`，Esc → `\x1b`，F1 → `\eOP`，Home → `\e[H`，End → `\e[F`，Shift+Tab → `\e[Z`；应用光标键模式四方向键 → `\eOA/OB/OD/OC`。失败原因：无。

### V8 快照拷贝（PASS，2/2）
已验证事实（`tests/v8_snapshot.rs`）：`RenderState::update` 取出的一帧拷贝为自有 owned 结构（行/单元格字符/前景色/宽度标记），继续写入并 `\x1b[2J` 清屏后，当前网格无内容而先前快照内容不变；`drop(term)` 后快照仍可读取且内容不变（无悬垂引用，Rust 所有权机制在编译期约束借用 + 运行时读验证）。耗时中位数（30 次取样，debug 构建，硬件相关仅记录）：80x24 = 1.22ms，200x60 = 7.37ms。失败原因：无。

## 未解决问题 / 已知限制

1. 产物字节非确定：多次构建 `libghostty-vt.a` 字节 hash 与大小不同（观测值 85,852,080 / 85,848,464 / 85,841,712 / 85,849,368 字节，ar mtime 已归零）。工程判断：差异来自 build-id 或 zig 内部非确定字段；功能等价以 V3/V4/V6/V7/V8 测试为准，未进一步定位字节级差异源。
2. V5 上游比对为代表性抽查（216 个上游 API 中抽 6 个 + 11 项语义逐项），非全量逐个比对；未发现"上游有绑定无"，但该结论置信度受抽查范围限制（工程判断）。
3. V4d 的 scrollback 行数用 `scrollback_rows()` 断言，未直接断言视口滚动偏移数值（绑定有 `viewport_row()` 但未在 V4 使用）。
4. V8 耗时为单环境 debug 构建数据，未测 release 基线，跨机器不可比。
5. V2 离线验证通过将网络请求指向本地不可达地址实现，非真实网络失效场景（已验证事实：该条件下构建仍成功）。

## 结论

V1~V8 各项检查通过。构建产物、离线可重复构建、下游链接、网格行为（文本/SGR/清屏/scrollback/宽字符/alternate screen）、11 项接口能力、PTY 交互、KeyEncoder 序列、owned 快照不变性均有可重复脚本证据（`checks/run_all.sh` 一次顺序运行 10 项检查通过）。
