# PHASE6B 进度（滚动记录，一行一条）

- [已验证事实] case27 在隔离会话（debug+DUMP 钩子，与 run_all.start_app 等价）跑 HEAD 得 519；完整 e2e（前 26 用例）稳定 2444；phase6a-done 完整 e2e 得 0 → 红是"前序状态 × 6B 改动"组合效应。
- [已验证事实] 回退 route_key 的"整串控制字符走编码器"后仍 2444 → route_key 排除。
- [已验证事实] request_inner_size 调用在 eperf! 之外无条件执行；两模式窗口几何均 1560×630 → 启动提示降级不是回归。
- [已验证事实] H3 实验（worktree 内回退 key_without_modifiers 的使用）第一次运行在 run_all 的 release 告警检查处被拦下（回退后遗留 unused import 触发 warning）→ 该实验无效，需同时清理 import 后重跑。
- [工程判断] 下一步：worktree 内回退 H3 时同步删除 `KeyEventExtModifierSupport` 相关 use 以消除告警，再跑完整 e2e（约 22 分钟）；若 case27 转绿则引入点=H3，最小修复方向为"仅在 Ctrl+符号键分支使用 key_without_modifiers"。
- [待验证假设] 2444 ≈ 120 列 × 一行，指向两次运行滚动偏移差一行；疑与 dump 钩子 I/O 时序 + 更少 stderr 写（启动提示降级）叠加有关。
