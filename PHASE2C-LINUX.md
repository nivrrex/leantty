# PHASE2C-LINUX：换行语义 / 选区滚动联动 / IME 预编辑 / 配置补齐 / 耗时校正

只记结论与证据。分级同 DECISIONS.md（已验证事实 / 工程判断 / 待验证假设）。

## 1. 完成矩阵

| 任务 | 交付内容 | 验收证据 |
|---|---|---|
| 1 复制文本换行语义 | 查证 libghostty-vt 折行 API（screen::Row::is_wrapped/is_wrap_continuation，screen.rs:296/300）；render state RowData 无 WRAP 变体，改走 Terminal::grid_ref(Point::Viewport)→row()→is_wrapped 逐行查询；FrameSnapshot 增 `wrapped: Vec<bool>`；extract_text 重写为"软折行拼接无分隔 / 硬换行插 `\n` / 行尾空白只作用逻辑行末尾 / 宽字符续格不重复 / 空行保留" | 5 单测（软折拼接、硬换行、混合、宽字符跨折行、空行保留）；e2e case19（100A 软折拖选读 PRIMARY 断言 A 连续 100 无分隔 + B1/B2/B3 含 `\n`）；vtz 探针 wrap_probe.rs |
| 2 选区与滚动联动 | 选区改存内容绝对坐标（行号=scrollback 顶起），新增 viewport_to_abs / abs_to_viewport / selection_viewport_region 纯函数；渲染按 offset 转换；跨视口提取经 VtTerminal::rows_text（Point::Screen 逐 cell 只读，不改变视口）+ extract_rows 统一格级语义；scrollback 截断时选区末行 clamp 到可见边界并记日志；拖选到上下沿 12px 自动滚动（auto_scroll_step 纯函数 + request_redraw 自驱动） | 4 纯函数单测（偏移往返、视口 clamp）；vt rows_text 单测（只读前后视口一致、软折标记）；e2e case20（上滚选 3 行→滚动往返→中键 Copy 重提取，P1=L055..L058=P2）、case21（下沿拖选自动滚动 40 行 > 视口 24） |
| 3 IME 预编辑渲染 | render/preedit.rs：PreeditOverlay{text,cursor_range}、layout_preedit/layout_end_position 纯函数、is_wide_char 内置区间表；Renderer trait 新增 `set_preedit`（默认空实现）；CpuRenderer 叠加绘制（底色覆盖、字形缩放、底部 2px 下划线、组合光标格反色）；app 每帧下发 overlay、Ime 事件经 ime_preedit_update 纯函数处理、set_ime_cursor_area 在 Preedit 期间指向组合末尾；APP_TEST_PREEDIT（仅 debug 粘性字段） | 7 preedit 单测（空串/ASCII/宽字符/超右边界换行/贴底平移/超整行截断/末尾定位）；4 ime_cursor 单测；4 ime_event 单测（Preedit/Commit/Disabled/Enabled）；e2e case22（叠加区差异 249 像素、下划线 maxima=1、dump 不含预编辑文本） |
| 4 配置字段补齐 | window.dynamic_title：vt 提供 Terminal::title()（OSC 0/2），app 每帧轮询与 last_title 比较后 set_title；标题初值=二进制名，false 时不更新。window.position：WindowAttributes::with_position(LogicalPosition)，Wayland 提示日志，越界交给 WM | e2e case23（位置 client(90,75) vs 请求 (60,50)，装饰偏移容差内；OSC 标题 T123 生效；dynamic_title=false 时 T999 不生效 → 标题固定 app） |
| 5 共享测试与耗时数据 | config path_priority 测试改 std::env::temp_dir()（平台无关，仅改该测试）；新增 [perf] bg_fonts_ms 钩子；release 空闲 5 次采样（见 §2） | cargo test --workspace 84 passed / 0 failed；clippy --all-targets -D warnings 0；release build 通过；run_all 76/76 连续 3 次全过 |

## 2. 启动耗时实测与 2B 数据勘误

测量条件（已验证事实）：release 二进制，Xvfb 1280x720 + openbox，Xvfb 启动后空闲 3s，每个配置连续 5 次冷启动（每次等窗口出现后 3s 采样），指标取 app 自身 `[perf] first_frame_ms`（首帧 present 耗时，相对进程启动）。

| 配置 | first_frame_ms（5 次） | 中位数 | 极差 | bg_fonts_ms（5 次） | 中位数 |
|---|---|---|---|---|---|
| 无配置文件（默认） | 175, 183, 194, 196, 209 | 194ms | 175–209（34ms） | 344, 344, 361, 375, 389 | 361ms |
| examples/linux.toml（含 黑体 fallback） | 171, 175, 202, 209, 215 | 202ms | 171–215（44ms） | 630, 692, 695, 695, 710 | 695ms |
| 旧口径对照（2B 基线，懒加载前） | — | 929ms（单次） | — | 同步阻塞，无独立时刻 | — |

`bg_fonts_ms` 为后台字体（CJK fallback）到位时刻（相对进程启动，仅打印一次）。linux.toml 下首帧 202ms、CJK 字体 695ms 到位，说明首帧不等待 CJK 加载（懒加载生效）。

**勘误（PHASE2B-LINUX.md §1/§4 中的三个数字口径不同，原文按规则不改，此处更正说明）：**

- `929ms`：2B 懒加载落地**之前** examples/linux.toml 的首帧（同步加载 19MB CJK 字体，fontdue from_bytes 实测 806.8ms 占主要部分）。
- `203ms`：2B 懒加载落地**之后** release 空闲环境下的首帧单次实测（当次采样值，非中位数）。本次 5 次采样中位数 202ms、极差 171–215ms，与该值一致。
- `823–946ms`：2B 交付期 e2e `case11` 的 `first_frame_ms`，为 **debug 二进制 + Xvfb 序列负载**（同一 run 内已起停多个 app、字体库冷热混合、openbox/xvfb 争用）口径，不是 release 空闲口径，不能与 203ms 直接比较。

结论（已验证事实）：release 空闲下，无配置与 examples/linux.toml 的首帧中位数分别为 194ms 与 202ms（极差 34ms / 44ms），CJK 字体在约 0.36–0.70s 后台到位，期间缺字显示占位方块。

## 3. Renderer trait 变更说明（对 Windows 的影响）

- 新增方法：`fn set_preedit(&mut self, overlay: Option<PreeditOverlay>)`，**带默认空实现**。Windows 侧共用同一 `Renderer` trait，未覆写也能编译；`CpuRenderer`（跨平台实现，非 Windows 专属文件）已覆写。
- 新类型 `PreeditOverlay { text: String, cursor_range: Option<(usize, usize)> }` 定义在 render crate（`render::preedit`），不含任何后端/平台类型，Windows 侧可直接复用。
- 未改动 `Renderer::render` 签名，未新增其他 trait 方法；未触碰 Windows 专属文件。
- 布局规则（DECISIONS 登记）：超出右边界整体换到下一行行首；贴底/越界时平移起点使其可见；总宽超过一整行时截断（不折行拆分）。

## 4. 未解决问题与偏差

- scrollback 截断后的选区重定位（工程判断→已知限制）：超 history（默认 10000）截断时，绝对行基准随窗口平移；当前实现仅把选区末行 clamp 到可见历史边界并打日志，未做内容锚点重定位，极端长会话下选区可能指向不同内容。e2e 不可达（history 10000），仅 clamp 分支覆盖。
- Preedit 缺字显示占位：APP_TEST_PREEDIT 注入 CJK 时，若后台字体未到位，叠加层按 .notdef 显示方块（case22 截图即此形态）；真实输入法下同样依赖 fallback 字体到位时间。
- Preedit 组合内多重下划线/分节样式未实现（统一单线下划线；候选窗由 IME 自身绘制）。
- 窗口大于屏幕时（如 120 列 × 13px = 1560px > 1280 屏幕），鼠标无法拖到窗口右缘之外的列：选区/滚动条拖拽的实际可达列受屏幕边界限制（e2e 通过 APP_COLS 或右下角坐标规避）。
- Xvfb 输入抖动（历史结论）：xdotool --window 定向按钮事件走 XSendEvent 被 winit 过滤，e2e 统一改 warp+XTest；ctrl+d keyup 偶发丢失以 start_app 的 keyup 清理消除。
- 选区与 IME 组合态同时存在时，预编辑绘制在终端光标处、选区高亮独立，不冲突（未做联合视觉优化）。

## 5. Windows 侧交接

- Renderer trait 新增 `set_preedit` 默认实现 → Windows 编译不受影响；若要显示 IME 组合串，只需在 Windows 渲染实现里覆写该方法并按相同布局规则绘制（render::preedit 纯函数可直接用）。
- 标题：`VtTerminal::title()` 为 vt 层接口，Windows 侧接窗口标题时同样可每帧轮询；dynamic_title=false 固定二进制名的语义已实现于 app 层。
- 窗口初始位置：`WindowAttributes::with_position(LogicalPosition)` 为跨平台 API；Windows 下由 WM 处理越界，不做钳制（与 Linux 一致）。
- config 字段：window.dynamic_title / window.position 已生效，scrolling.* / selection.* / mouse.* / keyboard.* 语义 Linux 已实现，Windows 共用同解析层。
- 本轮未改 win 分支及 Windows 专属文件（crates/pty/src/windows.rs、pty_windows.rs、scripts/windows/、docs/WIN-NOTES.md、vt-verify-win/、.cargo/config.toml 的 windows-gnu 段）。

## 6. 手动清单（≤3，Xvfb 无法覆盖）

1. 真实输入法候选窗位置：Preedit 期间 set_ime_cursor_area 指向组合末尾，需在真实 IME（fcitx/ibus/Windows IME）下确认候选窗跟随。
2. 真实 HiDPI 观感：Xvfb 固定 scale=1，缩放下的 Preedit 叠加、滚动条宽度、cell 度量需真机确认。
3. 字体观感：CJK fallback 到位前后的占位方块与最终字形，需人眼确认（默认配置无 fallback 时始终显示占位）。

## 7. 截图证据

- `scripts/e2e/out/phase2c-1-longline.png`：200 字符长行软折 2 行，全宽拖选高亮，PRIMARY 为单行连续 200 个 L（软折拼接）。
- `scripts/e2e/out/phase2c-2-selection-follow.png`：上滚后选区高亮仍落在 R01–R03 内容上；中键 Copy 重提取文本与滚动前一致（P1=P2=R01|R02|R03）。
- `scripts/e2e/out/phase2c-3-preedit.png`：Preedit 叠加（候选字 3 个占位字 + 下划线），dump 中不含该文本。
- `scripts/e2e/out/phase2c-4-title.png`：OSC 0 标题生效，标题栏显示 TTY-Phase2C。
