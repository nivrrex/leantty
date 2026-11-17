# BUGFIX-INPUT：app "输入无回显、回车无效" 缺陷修复

- 范围：缺陷修复 + app 运行级自动测试（Xvfb e2e）。只记结论与证据；分级同 DECISIONS.md。
- 环境：Xvfb :99 + openbox + xdotool + wmctrl（本机已装）；app release 构建同 README 环境变量契约。

## 1. 缺陷根因（逐段插桩实测，只记录实测）

e2e 插桩（临时 [e2e] 日志，定位后已删）逐段结论：

| 段 | 实测结论 |
|---|---|
| a winit KeyboardInput | 收到，16 条；`logical_key=Character("e")`、`text=Some("e")` 正常 |
| b KeyEncoder 输出 | **encoded 0B**（字母键经 KeyEncoder 编码需 event 的 unshifted codepoint，app 侧未设置时输出空字节） |
| c PTY writer | 写入"成功"但 0 字节（b 的下游结果），flush 无错误 |
| d PTY 读线程 | 正常（收到 shell 提示符与回车响应） |
| e 主线程 feed/snapshot | 正常（snapshot 非空、present ok） |
| f 事件循环唤醒 | ControlFlow::Wait 下键盘事件可唤醒；PTY 输出到达后无事件时循环不醒来（缺 EventLoopProxy 唤醒） |
| g softbuffer present | 正常（修复 surface 初始尺寸 panic 后） |

根因：b 点——字母键走 KeyEncoder 但 event 未设 codepoint，输出空字节，PTY 从未收到输入字符；用户症状"输入无回显、回车无效"（回车为命名键走 encoder 正常，但整行无字符）。

修复（根因级，非症状补丁）：
1. `route_key` 纯函数：`event.text` 优先直写 PTY；Ctrl/Alt 修饰组合与无 text 命名键走 KeyEncoder（其字节序列经 V7 断言验证）。
2. `Wake` trait + `ProxyWaker`（EventLoopProxy）：读线程每来一次数据 wake 一次事件循环（规格 f 点）。
3. softbuffer 初始尺寸：resumed 里 surface 创建后立即 resize（present 前必须，否则 panic；Xvfb 复现 "Must set size of surface before calling present()"）。
4. 渲染尺寸缓存 `last_size`：窗口销毁后不再调 `window.inner_size()`（winit 0.30.13 x11 window.rs:1248 GetGeometry unwrap panic）。
5. run() 收尾顺序先 close 后 join：reader 线程需 close kill 子进程后才 EOF，倒置将空等 5s 超时。

## 2. 新增测试清单与结果（全部实测通过）

单元/集成（cargo test --workspace，15 passed / 0 failed）：
- `app::default_shell` 平台回落（请求 1 遗留项，本周期前已提交）
- `route_key_text_priority_and_modifier_fallback`：text 优先/Ctrl/Alt/空 text/未映射键路由分类
- `encoded_key_bytes`：Enter/Tab/Backspace/Esc/方向键/Ctrl-C/Ctrl-D/Home/End/F1~F4 字节 + 应用光标键模式方向键 SS3 序列
- `waker_wakes_on_every_data_arrival`：channel + Wake 抽象，每次来数据都发出唤醒（5/5）
- `blit_clips_negative_offset_and_oversize_width_to_cell` / `blit_clips_oversize_height_to_cell`：邻格像素不变（请求 3，本周期前已提交）

e2e（scripts/e2e/run_all.sh，12 PASS / 0 FAIL）：
1. case1 hello_e2e 回显
2. case2 中文回显 + wide=2 宽格标记（xdotool type UTF-8 实测可用；printf 八进制路径保留为回退）
3. case3 abc+Backspace+d → abd
4. case4 sleep 100 + Ctrl-C + 标记串（证明前台进程被中断）
5. case5 Ctrl-D 后 app 5s 内退出
6. case6 关窗：wmctrl -c 在 run_all 序列环境下 WM_DELETE 消息丢失（工具层实测结论，见下）；e2e 改用 python3-xlib 直发标准 WM_DELETE，app 退出正常。
7. case7 resize 640x480 → cols=80/rows=30 且 stty size 输出一致
8. case8 vim 交替屏 on/off

## 3. case6 关窗工具链取证（已验证事实）

- 现象：run_all 序列环境下 case6（wmctrl -c 关窗）app 5s 未退出；单独探针/手动序列同命令稳定通过。
- 逐段实测：winit CloseRequested 插桩计数 0（消息未到达 app）；xev 监听无 ClientMessage；app 进程健康（wchan do_epoll_wait）；WID 经 xprop _NET_WM_PID 验证归属正确；wmctrl exit=0。
- 二分排除：case1 前序用例、windowactivate/focus、type x、xev 监听、时序 sleep 2 均逐一证伪（D 实验含 xev 仍通过）。
- 对照：同环境 python3-xlib 直发标准 WM_DELETE（WM_PROTOCOLS/WM_DELETE_WINDOW + CurrentTime）后 app 正常退出。结论：wmctrl -c 消息在序列环境下丢失，app 无缺陷；e2e 改用 wm_send.py 直发。

## 4. e2e 无法自动覆盖的项（原因）

- 真实 GPU/字体渲染观感（Xvfb 无 GPU，softbuffer 为 CPU surface；字体度量差异由 case2 宽格标记与像素断言覆盖，观感不可自动比对）
- 真实输入法（IME 组词上屏，属阶段 2 范围）
- 并发压力/长时间运行稳定性（阶段 1 范围外）

## 5. 用户本机确认清单（≤5 条，仅限 Xvfb 无法验证项）

1. 真实桌面会话起 `target/release/leantty`，键入中文（输入法）→ 组词上屏正常。
2. 真实 GPU 环境字符边缘/反锯齿观感无异常。
3. 拖拽窗口到极小尺寸（<1 列）→ 网格不崩溃、min(1) 兜底生效。
4. 粘贴大段文本（bracketed paste）→ 无乱码无丢失。
5. Ctrl-C 中断真实前台进程（如本机 `ping`）→ 与 case4 同行为。

## 6. 交付物与证据

- 提交：5dab029（根因修复+测试）、后续收尾顺序/clippy/e2e 提交见 git log；tag phase1-merged 之前。
- 证据文件：scripts/e2e/out/（每用例 dump+失败截图，运行时生成，未入仓库）；scripts/e2e/run_all.sh 一条命令全量。
- DECISIONS.md 登记：根因实测、修复设计、case6 工具链发现（xdotool windowclose 不发 WM_DELETE，xev 实证）、中文输入实测。
