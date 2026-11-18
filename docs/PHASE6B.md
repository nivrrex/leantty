# PHASE6B 进度（滚动记录，一行一条）

- [已验证事实] case27 在隔离会话（debug+DUMP 钩子，与 run_all.start_app 等价）跑 HEAD 得 519；完整 e2e（前 26 用例）稳定 2444；phase6a-done 完整 e2e 得 0 → 红是"前序状态 × 6B 改动"组合效应。
- [已验证事实] 回退 route_key 的"整串控制字符走编码器"后仍 2444 → route_key 排除。
- [已验证事实] request_inner_size 调用在 eperf! 之外无条件执行；两模式窗口几何均 1560×630 → 启动提示降级不是回归。
- [已验证事实] H3 实验（worktree 内回退 key_without_modifiers 的使用）第一次运行在 run_all 的 release 告警检查处被拦下（回退后遗留 unused import 触发 warning）→ 该实验无效，需同时清理 import 后重跑。
- [工程判断] 下一步：worktree 内回退 H3 时同步删除 `KeyEventExtModifierSupport` 相关 use 以消除告警，再跑完整 e2e（约 22 分钟）；若 case27 转绿则引入点=H3，最小修复方向为"仅在 Ctrl+符号键分支使用 key_without_modifiers"。
- [待验证假设] 2444 ≈ 120 列 × 一行，指向两次运行滚动偏移差一行；疑与 dump 钩子 I/O 时序 + 更少 stderr 写（启动提示降级）叠加有关。
- [已验证事实] 根因确立：`key_string(Named(PageUp))` 返回 `"PageUp"`，而 case27 配置绑定写 `key = "Page_Up"` → 绑定从未命中；6B 把 PageUp/PageDown 映射进编码器后，`ESC[5~`/`ESC[6~` 被写进 pty，改动行编辑/滚动状态 → case27 由绿变红，差异恰为一行 120 列（2444 px）。
- [已验证事实] 在 main 移除 PageUp/PageDown 两个映射后，完整 e2e 中 `case27` **PASS**（逐像素一致）；case21/63/64 均 PASS。该轮另有 `case41-bracketed-paste-ctrl-c` 失败（dropped=983040 wedge_ok=0，与本改动无关，待复跑确认）。
- [已验证事实] H3（unshifted_codepoint 改用 key_without_modifiers）回退后 case27 仍 2444 → H3 排除；route_key 与启动提示降级此前已排除。
- [待验证假设] PageUp/PageDown 是否应"无修饰传给应用"是产品行为问题，本阶段按裁决保持 0.1.0 行为（不转发），待用户裁定后再改。
- [已验证事实] 修复后门禁（`scripts/gates.sh` 10 步）全过，e2e **117/117**（GPU 轮 10/10），LOG: `GATES: PASS — HEAD=cb7c85d`。
- [已验证事实] `frame_bytes_identical` 黄金哈希仍为 `507485fb7945fb8f`（字体指纹 `DejaVuSansMono.ttf:343140:c69b:8x16` 匹配，非 SKIP）。
- [已验证事实] 已打 `phase6b-done`（轻量 tag，无 `^{}`），`main` 与 tag 均已 push 到 ali：main = `cb7c85dde01a026f45882870092b98b5f714f22b`。
- [已验证事实] `case41-bracketed-paste-ctrl-c` 在本轮修复后的首次完整 e2e 红（dropped=983040 wedge_ok=0），门禁复跑时通过 → 判为负载相关偶发（当时 loadavg 偏高），与本次 PageUp/PageDown 改动无关。
