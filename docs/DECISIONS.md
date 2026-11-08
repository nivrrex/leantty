# 决策记录

- 决策：vendor/libghostty-rs 置于仓库根（而非 vt-verify/vendor 下）。否决方案：保留嵌套路径——vt-verify 是冻结验证基线，骨架 crate 的 path 依赖不应指向被冻结目录内部。已验证事实：2a 阶段 git mv 提升后 v3/v4 回归通过。
- 决策：构建路径全部经环境变量（ZIG/GHOSTTY_SOURCE_DIR/GHOSTTY_ZIG_SYSTEM_DIR）传入，脚本未设置即报错。已验证事实：仓库内 grep 无 /root/Desktop 绝对路径残留。
- 决策：close() 契约为先释放 PTY 主端再回收子进程；读线程 join 带 5s 超时。依据：Windows 侧验证数据（先等子进程退出会使主端 EOF 延迟约 83s）；Unix 同样遵守。待验证假设：该 83s 数据在 Windows 侧的复现条件，Linux 侧未复现该延迟。
- 决策：FrameSnapshot 为完全自有类型，不含 libghostty 类型。依据：规格要求快照可被 render/app 独立持有。已验证事实：snapshot.rs 仅含自有结构体。
- 决策：骨架阶段 CpuRenderer::render 为占位实现，softbuffer/fontdb/fontdue 接线在实现阶段完成。依据：规格将渲染实现列入阶段（三）。待验证假设：Devuan 5 本机具备 softbuffer 所需的 X11/EGL 开发库（cargo check 时验证）。
- 决策：依赖版本不固定精确号（Cargo.toml 用宽松要求），与规格一致。已验证事实：portable-pty 0.8.1 与 vt-verify 基线一致。
- 未解决问题：winit/softbuffer/fontdb/fontdue 的实际解析版本以 Cargo.lock 为准（骨架提交时落盘）。
