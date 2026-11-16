# PHASE4R-LINUX：合并 3P / GPU 空闲跳过回归矩阵 / wgpu 环境变量审计 / DECISIONS 更正

分级：已验证事实 / 工程判断（含置信度）/ 待验证假设（含推翻条件）。

## 0. 摘要

| 任务 | 结果 |
|---|---|
| t1 合并 `phase3p-win-done` | 已合并 `ba36e44`（tag `phase4r-t1`），共用代码零改动 |
| t2 空闲跳过回归矩阵 | 9 类可见变化在 GPU 后端**全部仍会重绘**；空闲 CPU 占用 GPU 为 CPU 的 **0.13 倍**（tag `phase4r-t2`） |
| t3 wgpu 环境变量审计 | 12 个 `WGPU_*` 在本构建全部无效；门禁新增防护（tag `phase4r-t3`） |
| t4 DECISIONS 更正 | `from_env` 恒 `None` 的真实原因已改正（tag `phase4r-t4`） |

## 1. t1 合并证据（ls-remote 实测）

```
1e10fbc0293bb96f54718a183e855b33635db02d	refs/heads/main（合并前）
426f8254ba5dcd41c0bb2d287211a7f7e38ebe03	refs/heads/win
53789fef97c70f5235cc1b8693a335667de3d263	refs/tags/phase3p-win-done
ba36e44ad681bdc12fe81448c7b421a32e4c8a93	refs/tags/phase3p-win-done^{}
```

- 附注标签，解引用 = `ba36e44`；`refs/heads/win` = `426f825`，比它多一个仅改 `PHASE3P-WIN.md`
  的补记提交（+8/−1）。
- Windows 侧本轮只有 `ba36e44`（kit 缺陷修复），`git diff-tree` 核对 `crates/` 命中 **0**；
  改动集中在 `scripts/windows/tonight/`、`vt-verify-win/`、`docs/WIN-NOTES.md`、`PHASE3P-WIN.md`。
- 合并后相对 main 的共用/受控范围：`.gitignore`、`PHASE3O-WIN.md`、`PHASE3P-WIN.md`、
  `docs/WIN-NOTES.md`；`crates/` 零差异。

## 2. t2 GPU 空闲跳过的回归矩阵

脚本 `scripts/e2e/idle_repaint_matrix.sh`（默认不进 run_all）：对"不改变 VT 快照却可见的变化"
逐项触发，1 秒后截图与触发前比对（ImageMagick AE），最后检查空闲窗口帧数。
真实 NVIDIA GT 710 与 lavapipe（`APP_GPU_ALLOW_SOFTWARE=1` + `VK_ICD_FILENAMES=lvp_icd`）各一轮。

| 项 | 触发 | NVIDIA AE | lavapipe AE | 判定 |
|---|---|---|---|---|
| 1a | 拖选高亮 | 18837 | 18837 | OK |
| 1b | 点击清除选区 | 19110 | 19110 | OK |
| 3a | 滚轮上滚（拇指移动） | 7612 | 7588 | OK |
| 3b | 滚回底部 | 7885 | 7861 | OK |
| 6a | `OSC 11` 改背景色 | 4448 | 4460 | OK |
| 6b | `OSC 111` 复位 | 5118 | 5118 | OK |
| 7a | `ESC[?25l` 隐藏光标 | 5486 | 5474 | OK |
| 7b | `ESC[?25h` + `ESC[4 q`（下划线光标） | 5889 | 5901 | OK |
| 8 | `windowsize` 缩放 | 185814 | 185826 | OK |
| — | 空闲窗口 `frames_window` | **0** | **0** | OK |

**未在本脚本内、由既有用例覆盖的项**：

| 项 | 覆盖方式 | 说明 |
|---|---|---|
| 2 预编辑（`APP_TEST_PREEDIT` / `_CLEAR_MS`） | `case30` | 该钩子仅 debug 构建存在，故用 debug 二进制跑；`Ime`/`Preedit` 事件本身会置 urgent |
| 4 焦点变化改变光标样式 | **本实现无此行为** | 代码中无 focus→光标样式的绘制逻辑，故无从漏画 |
| 5 Expose | `case31` | 覆盖窗方式；`RedrawRequested` 置 `os_redraw_pending`+`urgent` |
| 9 反色/粗体等纯快照变化 | `case26/27/34/35/36`、`gpu_parity` | 均由 PTY 输出驱动，必然产生新快照 |

**结论**：4Q t3 的空闲跳过没有造成漏画——所有"不改变快照"的可见变化都经由
`urgent_render`（鼠标/键盘/IME/OS 重绘/缩放）或 PTY 字节（OSC、光标序列）触发，均不被跳过。
故本轮**没有修改产品代码**，脚本作为回归保护保留。

### 2.1 空闲耗电代理（release，:99，空闲 20 秒）

| 后端 | `utime+stime` 增量 | voluntary_ctxt_switches | 最后窗口 `frames_window` |
|---|---|---|---|
| GPU | **36 tick（360ms）** | 383 | **0** |
| CPU | **267 tick（2670ms）** | 2490 | **623** |

判据"GPU 空闲不高于 CPU 的 1.5 倍"：**0.13 倍 → 通过**。

> 顺带暴露的问题（已验证事实）：**CPU 后端空闲仍在空转**（623 帧/5 秒、约 13% 单核）。
> 4Q t3 的空闲判定只加在 GPU 路径；CPU 后端"渲染但不提交"（damage 为空才提前返回）。
> 本轮按任务边界未改（改它需要确认 CPU 侧 `snap.is_clean()` 判定同样安全，属另一项工作）。

## 3. t3 wgpu 环境变量审计

### 3.1 机制

`wgpu-types-30.0.2/src/env.rs`：

```rust
pub fn var(_key: &str) -> Option<String> {
    #[cfg(feature = "std")] return std::env::var(_key).ok();
    #[cfg(not(feature = "std"))] return None;
}
```

本项目以 `default-features = false` 引入 wgpu（`wgpu-types` 的 `default = ["std"]` 未启用），
因此**所有经此函数读取的环境变量一律为 `None`**。

### 3.2 审计表（12 个变量，全部经 `env::var`）

| wgpu API | 读取的变量 | 本仓库是否调用 | 本构建行为 | 用户可见影响 |
|---|---|---|---|---|
| `Backends::from_env` | `WGPU_BACKEND` | 否（4Q 起用自有 `parse_backends_spec`） | 恒 `None` | 无（自有解析器负责） |
| `PowerPreference::from_env` | `WGPU_POWER_PREF` | 否（按 Discrete>Integrated 规则选） | 恒 `None` | 设了也无效 |
| `Dx12Compiler::from_env` | `WGPU_DX12_COMPILER` | 否 | 恒 `None` | 无效 |
| Dx12 选项（4 个） | `WGPU_DX12_AGILITY_SDK_{PATH,REQUIRE,VERSION}`、`WGPU_DX12_PRESENTATION_SYSTEM`、`WGPU_DX12_USE_FRAME_LATENCY_WAITABLE_OBJECT` | 否 | 恒 `None` | 无效 |
| `Gles3MinorVersion::from_env` | `WGPU_GLES_MINOR_VERSION` | 否（本构建无 GL 后端） | 恒 `None` | 无效 |
| GL 选项（2 个） | `WGPU_GL_DEBUG_FNS`、`WGPU_GL_FENCE_BEHAVIOR` | 否 | 恒 `None` | 无效 |
| Noop 后端 | `WGPU_NOOP_BACKEND` | 否 | 恒 `None` | 无效 |

我们实际使用的都是常量：`wgpu::InstanceFlags::empty()`、`wgpu::BackendOptions::default()`。
**结论：除 `WGPU_BACKEND`（本项目自己解析）外，没有任何被依赖却静默失效的 `from_env`。**

### 3.3 门禁防护（`scripts/gates.sh` 第 8 步）

- crates/ 下不得出现 `(Backends|InstanceFlags|Dx12Compiler|Gles3MinorVersion|PowerPreference|BackendOptions)::from_env`
  或 `wgpu::*::from_env`（当前为空，实测通过）；
- `cargo tree -e features` 中不得出现 `wgpu-types feature "std"`（当前为空）——若将来启用 std，
  门禁会失败并提示需要同时重新审查环境变量行为。

### 3.4 单测补充

4Q 的 6 条解析单测已覆盖 3P 观察到的组合（尾随空格 `"dx12 "`、大小写 `DX12`、逗号列表
`vulkan,dx12`、以及 `vk`/空/乱码）；本轮再加 1 条 `rejects_literal_quotes_from_shell_typos`
（若引号真的进了变量值，必须明确报错而非静默忽略）——共 7 条。

## 4. t4 DECISIONS 更正

- 4Q t2 条目中"变量没进进程环境/值被吞掉"的推测已在原文以**删除线**保留，并在其后追加
  更正块：真实原因是 `wgpu-types` 在 `default-features = false` 下 `env::var` 恒返回 `None`
  （`src/env.rs`），故 `Backends::from_env()` 对任何值都是 `None`；依据为 Windows 3P 的独立探针
  （同版本 wgpu 30.0.2 上 `from_env()` 始终 `None`，`std::env::var` 正常）与源码。
  标注为已验证事实，注明更正日期（2026-11-16）与来源（`PHASE3P-WIN`）；
  "4Q 自有解析器已使其生效"这一事实保留；未改动其它历史条目。
- 新增：Windows 的 `backend="auto"` 继续保持 CPU；翻转条件 = (1) WARP 下 DX12 路径通过 3Q
  的回归矩阵；(2) 4R t2 的回归矩阵在真实 Windows GPU 上全部通过；(3) 至少再有一份不同真实
  GPU 的反馈。

## 5. Windows 3R 交接

本轮 **t2 未改共用代码**（矩阵全部通过，空闲跳过无需修补），但请 Windows 侧用同一矩阵覆盖：

1. 在真实 Windows GPU 上按 `scripts/e2e/idle_repaint_matrix.sh` 的 9 项逐条验证（脚本是 POSIX sh
   + xdotool，Windows 侧需按其等价操作重写，或参考其判据手工执行）：
   选区/清除、滚轮与回底、OSC 11/111、光标显隐与形状、窗口缩放、以及空闲帧数收敛。
2. 用 WARP（`APP_GPU_ALLOW_SOFTWARE=1` + `backend="gpu"`）复跑同一矩阵（这是 auto 翻转条件之一）。
3. 确认 `WGPU_*` 变量在 Windows 上同样无效（本构建无 std 特性），有效变量只有
   `WGPU_BACKEND`/`WGPU_ADAPTER_NAME`/`APP_GPU_ALLOW_SOFTWARE`/`APP_PERF_FRAMES`。
4. 空闲耗电：Windows 上可用 `Get-Process` 的 CPU 时间增量与 `Get-Counter` 做等价代理，
   判据仍是"GPU 空闲不高于 CPU 的 1.5 倍"。

## 6. 未解决问题

1. **CPU 后端空闲仍空转**（623 帧/5 秒、约 13% 单核）：属已知低效，未修（超出本阶段边界）。
2. **回归矩阵未在 Windows 上执行**：本阶段全部结论来自 Linux（真实 NVIDIA + lavapipe）。
3. **`case39` 的一次 flake**：gate 首轮出现 `loop_gap=192ms`（单独复跑 6ms、bytes 精确），
   判定为环境抖动，重跑 gate 通过；未放宽其阈值。

## 7. 手动清单（≤3）

1. Windows 上用 WARP 复跑 t2 的矩阵（同时满足 auto 翻转条件 1、2）。
2. 观察 Windows GPU 后端空闲时的 CPU/GPU 占用，确认与 Linux 的"0 帧"一致。
3. 若要处理 CPU 后端空闲空转，请确认是否允许把 4Q t3 的空闲判定同样应用到 CPU 路径。
