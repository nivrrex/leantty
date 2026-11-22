# leantty 0.2.10

针对"真机 GPU 路径偶发卡顿/崩溃"的可用性与可观测性修复。

## 修复

- **GPU 的 `present_ms` 一直是 0**：GPU 路径此前固定 `present_us.push(0)`（注释写"present 已在
  render_ms 内"），所以"窗口被遮挡导致 present 阻塞十几秒"在 perf 行里看不见，只会让 `render_ms`
  变大、无法归因。现在 `get_current_texture`（acquire）与 `queue.present` 分别计时，perf 行新增
  `present_ms_max`，可直接看出单帧最长阻塞。
- **遮挡期间不再渲染**：此前完全没处理 `WindowEvent::Occluded`。FIFO 交换链下窗口被完全遮挡时
  合成器不再消费帧，`get_current_texture` 可能长时间阻塞（这正是"输出卡住十几秒"的典型机制）。
  现在记录遮挡状态，遮挡期间跳过渲染与 present（PTY 仍然照常喂入、不丢数据），窗口重新可见时
  立刻补一帧。该行为与 alacritty 一致（其 `draw()` 在 `occluded` 时直接返回）。

## 核实过、未改动的点

- **`backend = "auto"` 对软件适配器的过滤是有效的**：`Gpu::new` 会跳过 `DeviceType::Cpu`
  （并打印 `[perf] gpu_skip_software`），只保留真实 GPU；`auto` 永远忽略 `LEANTTY_GPU_ALLOW_SOFTWARE`。
  在 Xvfb 上实测 `auto` 选到的是真实 `NVIDIA GeForce GT 710`——因为 Vulkan 在 Xvfb 下依然枚举得到
  真实设备，这不代表过滤失效。
- **present 模式仍为 `Fifo`**（与 alacritty 默认一致）。是否改用 `AutoNoVsync`/`Immediate` 需先拿到
  真机数据：用新加的 `present_ms_max` 对比遮挡/非遮挡两种情况后再决定，避免为了平滑而引入撕裂。
- 图集缓存（命中不重传）、2 倍增长、每帧实例提交、`coalesced` 计数均核实正常。

## 验证

- `cargo test --workspace` 42 个测试二进制全绿；clippy（Linux 与 `x86_64-pc-windows-gnu`）0 告警。
- Xvfb + 真机 GPU 实测：修复前 `present_ms(med=0 p95=0)`，修复后 `present_ms(med=19 p95=21) present_ms_max=21`；
  连续 30 MB 输出下遮挡与非遮挡两组的帧率、`coalesced` 均正常。
- **未在真实桌面会话（`:0`）与 Windows 实机验证**：遮挡事件在 X11 下不投递（实测遮挡窗口后仍出帧），
  该修复主要面向 Windows/Wayland。

## 资产

| 文件 | 大小 | sha256 |
|---|---|---|
| `leantty_0.2.10_amd64.deb` | 见 SHA256SUMS | 见 SHA256SUMS |
| `leantty-0.2.10-windows-x86_64.zip` | 见 SHA256SUMS | 见 SHA256SUMS |
