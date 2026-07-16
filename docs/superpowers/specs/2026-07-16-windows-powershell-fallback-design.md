# Windows PowerShell 启动回退设计

## 目标

修复 Quotio v0.7.0 在部分 Windows 机器启动商店版 Codex 时出现的 `调用 PowerShell 失败: program not found`。v0.7.1 只处理 PowerShell 可执行文件解析与回退，不改变开机自启、Codex 配置注入、商店应用激活顺序或代理生命周期行为。

## 根因

商店版 Codex 位于 `WindowsApps` 时，Quotio 会通过 PowerShell 执行 `shell:AppsFolder` 激活。当前实现固定调用裸命令 `powershell`，依赖 Quotio 进程继承的 `PATH`。当系统精简了环境变量、Windows PowerShell 目录不在 `PATH`，或者机器只安装了 PowerShell 7 时，Rust 在创建子进程阶段返回 `NotFound`，并显示 `program not found`。

同一文件中的 Appx 探测、Codex 进程清理和 CLI 终端启动也存在相同的硬编码调用，因此不能只修商店激活的单一调用点。

## 方案选择

采用统一 PowerShell 候选解析与执行方案。

不采用只写死一个系统路径的方案，因为 Windows PowerShell 组件可能被移除，而 PowerShell 7 仍可用。不在本补丁中改用 Windows ShellExecute，因为那需要引入新的 Windows API 绑定并重写商店入口激活，改动范围不适合补丁版本。

## PowerShell 候选顺序

Windows 下按以下顺序尝试：

1. `%SystemRoot%/System32/WindowsPowerShell/v1.0/powershell.exe`。
2. `%SystemRoot%/Sysnative/WindowsPowerShell/v1.0/powershell.exe`，仅路径实际存在时加入，用于兼容 32 位进程访问 64 位系统目录。
3. `powershell.exe`，交给 Windows 的标准可执行文件搜索处理。
4. `%ProgramFiles%/PowerShell/7/pwsh.exe`。
5. `pwsh.exe`，兼容只在 `PATH` 中提供 PowerShell 7 的机器。

候选列表去重。绝对路径不存在时直接跳过；命令启动返回 `ErrorKind::NotFound` 时继续下一个候选。遇到权限错误等其它创建进程错误时立即返回，并带上候选路径。第一个成功启动的解释器负责执行脚本；脚本返回非零退出码时保留现有 stderr 解析，不再尝试另一解释器，以免掩盖真正的 PowerShell 脚本错误。

所有候选都不存在时，错误明确列出已尝试的解释器，并提示检查 Windows PowerShell 或 PowerShell 7，而不是只显示 `program not found`。

## 接入范围

`crates/quotio-core/src/codex_launch.rs` 中以下路径统一复用同一解析逻辑：

1. `detect_codex_via_appx`：通过 Appx 包探测 Codex 安装位置。
2. `run_powershell`：商店入口探测、`shell:AppsFolder` 激活和 PID 探测。
3. `close_codex_app`：清理由 Node 启动的 Codex CLI 进程。
4. `launch_codex_cli`：打开 Windows Terminal 或命令提示符时，使用实际解析出的 PowerShell 程序，而不是固定字符串。

原有 `CREATE_NO_WINDOW`、`-NoProfile`、`-NonInteractive` 和脚本参数保持不变。

## 错误与回滚

- 找不到某个候选：继续尝试下一个。
- 所有候选都找不到：返回可操作的缺失说明。
- 解释器存在但脚本失败：返回脚本 stderr 或退出码。
- Codex 启动失败：沿用现有上层回滚，释放绑定账号并还原 Codex 配置备份。
- 进程清理属于 best-effort；PowerShell 不可用时仍不能阻止 Quotio 退出或恢复配置。

## 测试

新增 Windows 回归测试覆盖：

1. 系统 Windows PowerShell 绝对路径排在 `PATH` 命令之前。
2. Windows PowerShell 缺失时仍包含并尝试 `pwsh.exe`。
3. 候选路径去重且保持稳定顺序。
4. 第一个候选返回 `NotFound` 时继续执行第二个候选。
5. 非 `NotFound` 的创建进程错误不会被后续候选掩盖。
6. 现有 WindowsApps 路径识别、PowerShell 字符串转义和 Codex 启动相关测试继续通过。

完成实现后运行目标测试、`cargo test -p quotio-core`、`cargo check --workspace`、前端测试和前端生产构建。随后更新四处应用版本号、README 和 `CHANGELOG.md` 为 v0.7.1，提交、打标签并推送，确认 GitHub Actions 三平台发布成功。
