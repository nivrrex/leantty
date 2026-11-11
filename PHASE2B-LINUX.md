# PHASE2B-LINUX：启动性能 / 选区与剪贴板 / 鼠标上报 / 滚动 / IME

只记结论与证据。分级同 DECISIONS.md（已验证事实 / 工程判断 / 待验证假设）。

## 1. 完成项（全部自动验收通过）

- 任务1 启动性能：基线实测 release 首帧无配置 339ms、examples/linux.toml 929ms；fontdue from_bytes 19MB CJK release 806.8ms（debug 6.1s）一次性成本。懒加载：主字体同步、fallback/系统兜底后台线程（render::load_font_list 自由函数+channel+EventLoopProxy 唤醒）；未到位码点统一 U+25A1 占位（全链缺失同路径不 panic）；[perf] first_frame_ms 日志（static START OnceLock）。首帧 929→203ms（任务1 实测）；本轮 e2e 五次采样 823-946ms（Xvfb 负载波动区间，含 CJK 后台加载期）。background_load 2 单测（请求→占位→到位→重绘替换序列+空列表失败路径）。
- 任务2a 选区与剪贴板：clipboard.rs（ClipboardKind；trait Clipboard get/set；ArboardClipboard；InjectClipboard 内存假实现；trait 非 Send 文档注释——arboard X11 连接主线程持有）；paste.rs（normalize_paste：换行统一 \r、?2004 包裹 ESC[200~/201~ 并过滤内容中 ESC[201~ 防注入、PASTE_CHUNK=64KB+paste_chunks 字符级分块）；selection.rs（CellPos/Selection{anchor,focus}/region/contains/word_region semantic_escape_chars 边界/extract_text 去行尾空白+SpacerTail 不重复+行间不插换行）；app Error Clipboard 变体；纯函数单测。
- 任务2b 接线：拖选/双击选词/三击选行/Shift 扩展（<300ms 同格计数）；选区定型写 PRIMARY（save_to_clipboard 时兼写 CLIPBOARD）；render CpuRenderer.set_selection 选区格反色（SpacerTail 不高亮，单测含清除后恢复）；mouse.bindings/keyboard.bindings 查表执行（同键后者覆盖；空配置回落内置默认 Middle=Copy/Right=Paste/Ctrl+Shift+C/V）；鼠标上报优先级（非 Shift 且 is_mouse_tracking 时 encode_button 写 PTY，Shift 绕过）；粘贴执行（读 CLIPBOARD/PRIMARY→normalize_paste→?2004 包裹→写 PTY，≥1MB 经 pending 队列分块每帧一块不阻塞事件循环）；坐标含 padding/缩放（pixel_to_cell 物理像素→网格）。
- 任务3 鼠标上报：MouseEncoder encode_button Press/Release/Motion+Button Four/Five 滚轮编码；SGR 1006 由 vt MouseEncoder 按模式自动选（?1006 开时 SGR 格式，e2e case16 断言 ^[[<0; 序列）；备用屏滚轮→方向键（scrolling.alt_screen_scroll 配置项，默认 true；ArrowUp/Down×multiplier）；非备用屏 scroll_viewport Delta。
- 任务4 滚动：scrolling.multiplier（默认 3.0，滚轮/PageUp 均乘）；PageUp/Down/Top/Bottom 绑定（run_action Scroll* 维护 offset+follow）；scrollbar.rs 纯函数（scrollbar_geometry 拇指占比/track_y_to_offset 逆映射，4 单测含 offset clamp 防御）；render draw_scrollbar（右缘 4px 轨道+拇指，通道平均近似半透明）；拖拽（左键命中右缘条带→scrollbar_drag 优先于选区+CursorMoved 实时 Row 定位）；auto-follow 状态机（follow_bottom+offset_from_top；实测 vt 上滚后 feed 不自动回底→app 层维护，scroll_probe.rs 探针+DECISIONS 登记；feed 后 follow 时 Bottom 回底）；e2e case14 滚轮上滚历史行+下滚回底、case18 条带拖到顶（DRAG_1 可见+DRAG_50 离开）。
- 任务5 IME：ime.rs（normalize_ime_commit 换行归一+preedit_display 空串清除/clamp 保序，3 单测）；set_ime_allowed(true) 窗口创建后无条件；Ime::Enabled/Preedit（存字段）/Commit（归一写 PTY）/Disabled（清空）；set_ime_cursor_area 每帧跟随快照光标格像素矩形；e2e case17 冒烟（Xvfb 无 IME 总线，事件路径不可达登记限制）。
- 根因修复（e2e 取证）：reader EOF 退出前 wake 主线程（否则事件循环无事件可等→about_to_wait 不触发→try_wait 轮询丢失→app 挂起；case11 偶发 5s 未退出根因；回归单测 eof_wakes_event_loop_before_exit）。
- e2e（run_all 54/54 PASS，5 连稳定）：case12 拖选+PRIMARY 写+右键 Paste 执行、case13 括号粘贴 ?2004 包裹（cat -v 断言 ^[[200~…^[[201~）、case14 滚轮上下滚、case15 备用屏滚轮→方向键、case16 SGR 1006 上报、case17 IME 冒烟、case18 滚动条拖拽；既有 case1-11 全量回归。
- 验收：cargo test --workspace 58 passed / 0 failed；clippy --all-targets 0；build --release 通过；run_all 54/54 PASS（5 连）；examples/linux.toml 实操序列截图 scripts/e2e/out/phase2b-1-cjk.png（汉字渲染）、-2-selection.png（选区反色）、-3-copy.png、-4-paste.png（Shift+Insert 粘贴执行回显）、-5a/-5/-6（滚轮上滚历史区+滚动条轨道+拇指）。

## 2. 剪贴板库评估结论（arboard 3.7.0）

- 选 arboard 优于自写 X11（DECISIONS 登记）：成熟库、PRIMARY+CLIPBOARD 同接口、Windows 共用 Clipboard trait（阶段1 接口不变）；自写 X11 所有权语义成本高（set 后持有者存活至进程退出）。
- features：X11 为核心特性（无 x11 feature 开关，cargo add 报错验证）；default-features=false 去 image-data 链（文本剪贴板不需要图像，依赖最小化）。
- 线程安全：trait 不加 Send（arboard Clipboard 非 Send，仅主线程调用，文档注释声明）。

## 3. 配置字段支持矩阵更新（相对 2A）

| 字段 | 2A 状态 | 2B 状态 |
|---|---|---|
| scrolling.history | 仅解析 | 已生效（vt scrollback_rows 反映） |
| scrolling.multiplier | 仅解析 | 已生效（滚轮/方向键倍率） |
| scrolling.alt_screen_scroll | 未实现 | 已生效（备用屏滚轮→方向键开关，默认 true） |
| selection.save_to_clipboard | 仅解析 | 已生效（选区定型时兼写 CLIPBOARD） |
| selection.semantic_escape_chars | 仅解析 | 已生效（双击选词边界） |
| mouse.bindings / keyboard.bindings | 仅解析+表单测 | 已生效（查表执行，空配置回落内置默认） |
| shell.args | 仅解析 | 未变（Pty::spawn trait 不含 args，改 trait 属停止条件） |
| window.position / dynamic_title / opacity | 仅解析 | 未变（阶段 3+ 范围） |

## 4. Windows 交接

- Clipboard trait 为 Windows 共用接口（app 内抽象）；arboard Windows 后端同 get/set，win 分支接入时仅换 ArboardClipboard 实现，trait 调用不变。
- config 字段解析全平台同（config crate 不依赖平台）；scrolling.alt_screen_scroll/multiplier、selection.*、mouse/keyboard.bindings Windows 同语义。
- win 分支及 Windows 专属文件（crates/pty/src/windows.rs、pty_windows.rs、scripts/windows/、docs/WIN-NOTES.md、vt-verify-win/）本轮未动。
- IME：winit set_ime_allowed/set_ime_cursor_area 跨平台同 API；Windows IME 事件路径同代码（Win32 IME 由 winit 后端适配）。

## 5. 手动清单（≤3，需真实环境）

1. Windows 构建+剪贴板 PRIMARY/CLIPBOARD 行为实测（arboard Windows 后端；Linux 侧已由 xclip 跨进程读验证）。
2. 真实输入法合成事件（Preedit 组合中间态绘制；Xvfb 无 IME 总线，事件路径仅单测+冒烟覆盖）。
3. GPU 渲染（阶段 3 范围，本阶段不涉及）。

## 6. 未解决问题与偏差

- 选区滚动联动未接（DECISIONS 登记）：选区以网格坐标存储，用户上滚后选区不随视口偏移渲染（任务 2 遗留限制，阶段 3 前补 offset 联动）。
- 折行与真换行在快照不可区分（extract_text 行间不插分隔符；多行选区文本为行拼接，e2e/用例设计已规避）。
- IME Preedit 组合中间态仅存储不绘制（无 overlay 组件，超出阶段范围）。
- Xvfb 序列环境输入抖动（xdotool --window 定向老版本走 XSendEvent 被 winit 过滤；ctrl+d 偶发丢）：e2e 侧已稳健化（按钮事件走 warp+XTest、ctrl 系列重发、start_app 进程退出重试、wait_window zombie 快败、run_all 开头清 Xvfb/openbox 残留）；app 侧无缺陷（case6/case11 取证结论）。
- case11 首帧毫秒数依 Xvfb 负载波动（实测区间 203-946ms）；懒加载保证首帧不阻塞于 CJK from_bytes（806.8ms 一次性后台成本）。
- 良性日志保留：hinting unsupported、system fallback unavailable、request_inner_size None（Xvfb 噪音，2A 已登记）。
