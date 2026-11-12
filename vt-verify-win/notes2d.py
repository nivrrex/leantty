import io
p='C:/dev/tty/docs/WIN-NOTES.md'
s=io.open(p,encoding='utf-8').read()
old="- 请求 6（P1，功能完整性）："
i=s.find(old)
assert i>0
add = """- 请求 7（2D 复验）：crates/config/tests/config.rs 的 path_priority_cli_env_default 在 Windows 仍失败——测试设置 XDG_CONFIG_HOME 后断言 default_config_path() 落在其下，但 Windows 实现走 APPDATA。实际断言：path "C:\Users\Admin\AppData\Roaming\config-<hash>\config-<hash>.toml" should start with "C:\Users\Admin\AppData\Local\Temp\xdg-test"。Linux 侧只修了 temp 目录构造，请按平台分支或跳过该断言。状态=已验证事实，未擅改（共享测试）。
- 事实补充（2D，已验证）：ConPTY 把控制台宽度处的折行**物化为 CRLF**（probe_ptyin：120 列下 200 个 A 到达为 A*120 CRLF A*80），故 2C.1 的软折行拼接在 Windows 经真实 shell 不可触发；功能在 vt 层正常（probe_wrap 对同字节序列报 wrapped=true）。
- 事实补充（2D，已验证）：请求 6 的 1MB 粘贴排空缺陷已被 2D.2 修复在 Windows 生效——静止粘贴精确 1048576 字节（约 7s），无需额外事件；请求 6 可关闭。
- 事实变更（2D，已验证）：PHASE2B-WIN 记录的 pwsh 7.6.6 ConPTY FailFast 本轮**未复现**：flags 0/1/2/4/6/7 × 80x24/120x30/200x50 共 18 组合全部存活且无 totalWidth；app 以 shell=pwsh.exe 启动正常。2B 与本轮之间仅发生一次本机重启（2B 期间另有多个残留 app/ConPTY 实例）。待验证假设：崩溃依赖会话级 conhost 状态。未据此改任何 spawn 参数。
"""
s = s[:i] + add + s[i:]
io.open(p,'w',encoding='utf-8',newline='\n').write(s)
print('ok')
