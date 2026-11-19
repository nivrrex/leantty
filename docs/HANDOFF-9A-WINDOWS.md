# Windows 侧接手步骤（9A：字形修复 + 启动方块期优化）

基线：Linux 侧 `rel-0.2.2`，tag **`phase9a-done2`**（GPU 字形修复）与 **`phase9a-perf`**（启动优化）。
Windows 现有的 `leantty-0.2.2-windows-x86_64.zip`（`f80d82fe…`）**不含这两批改动**，必须重打。

## 0. 先合并（不改 Linux 侧）
1. 从 `phase8c-win-done`（或当前 Windows 分支）拉新分支，合并 `phase9a-done2`，再合并 `phase9a-perf`。
   预期冲突：`docs/DECISIONS.md`（双方各自追加，保留两段）；`crates/render/src/gpu/mod.rs` 若有 Windows 侧本地改动则按"两处都在"处理。
2. `git diff --stat phase8c-win-done..HEAD -- crates/` 核对只有：
   `crates/render/src/gpu/atlas.rs`（新增 `clear(queue)`）、`crates/render/src/gpu/mod.rs`（清 atlas + `add_background_fonts_shared`）、
   `crates/render/src/lib.rs`（`load_font_list_shared` + `add_background_fonts_shared`）、`crates/app/src/lib.rs`（后台字体线程提前到建窗前 + 共享通道）、
   `scripts/e2e/run_all.sh`（case81 三项断言）、`CHANGELOG.md`、`README*`、`docs/*`。
3. **不要**把 `main` 合进来（main 含未发布的 8B 观测改造）。

## 1. Windows 侧必须验证的三件事
1. **GPU 字形**：`backend = "gpu"` + 配置 CJK fallback（如 `HYQiHei`/`Microsoft YaHei`），
   冷启动后中文提示符必须是真实字形，不能是方块；`--renderer cpu` 与 `gpu` 在同一窗口尺寸下截图应一致。
2. **启动方块期**：期望 `bg_fonts_ms` − `first_frame_ms` 明显小于改前（Linux 实测 0.65 s → 0.31 s）；
   Windows 的 `load_system_fonts`（注册表/字体目录扫描）通常比 Linux 慢，**若方块期 >0.5 s 属预期**，记录实际值即可。
3. **回归**：字体并入后必须有一次重绘（既有的 `urgent_render`/`request_redraw` 路径），
   Windows 上"3 秒自动恢复"的老症状应消失；用同一 `case81` 逻辑（含 CPU/GPU 像素一致断言）确认。

## 2. 重打 Windows zip（沿用 8D 的流程）
1. `cargo build --release --target x86_64-pc-windows-gnu`（工作区，含 `--tests` 的 check 由 gates 覆盖）。
2. 打包脚本与 8D 一致：zip 名 `leantty-0.2.2-windows-x86_64.zip`，**解压后 6 项内容与之前一致**
   （`leantty.exe`、`config.toml`、`README.md`、`README.zh-CN.md`、`LICENSE`、`THIRD-PARTY-NOTICES.md`）。
3. 回传：zip 的**字节数与 sha256**，并附 `leantty.exe` 的 sha256。Linux 侧据此更新
   `/root/dev/leantty-release-assets-9a2/` 与 `SHA256SUMS`。

## 3. 交付格式（回给 Linux 侧的信息）
```
zip: leantty-0.2.2-windows-x86_64.zip  <字节数>  <sha256>
exe: <sha256>
合并后 HEAD: <sha>
gates/CI 结果: <命令与结论>
case81: PASS/FAIL（含 CPU vs GPU 像素差）
bg_fonts_ms / first_frame_ms（Windows 实测）: <值>
未验证项: <列出>
```

## 4. 已知限制（照抄，不要"顺手修"）
- `case52`（SIGKILL 后 <=3s 关闭）在 Linux 无区分力（杀会话首进程必然 pty 挂断），Windows 上才有意义。
- deb 因 `changelog.Debian.gz` 时间戳不可复现（包内二进制可复现）。
- Windows 上 `should_free_console` / `GetConsoleProcessList` 的实机行为此前一直标"未实机验证"，本次不扩大范围。
