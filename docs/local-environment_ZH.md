# 本机开发环境

本文件只记录**当前开发机**的工具链、路径和已知坑，作用是让下一次会话能复现同样的
构建与验证行为。它不代表产品支持范围，也不能作为“只支持 Arch Linux”的依据；支持
平台见 `GOALS.md` 与 `PLANS.md`。

## Linux（Arch + Wayland）

- 操作系统：Arch Linux（滚动发行版）。系统包用 `pacman` 安装，例如
  `sudo pacman -S ffmpeg`；不通过脚本直接下载系统级二进制。
- Rust 工具链：由 `rustup` 管理，版本由 `rust-toolchain.toml` 固定（当前 `1.99.1`，
  含 `rustfmt` 与 `clippy`）。缺组件时用 `rustup component add rustfmt clippy`。
- 工具链升级：升级 `rust-toolchain.toml` 是独立变更，不与功能改动混在一起。
- FFmpeg / FFprobe：使用系统包（`/usr/sbin/ffmpeg`、`/usr/sbin/ffprobe`；
  `ffprobe` 由 `ffmpeg` 包提供，不是独立包），项目不自带二进制。滚动发行版会改变
  实际版本，因此测试不得假设固定版本行为。
- 桌面会话：Wayland（`XDG_SESSION_TYPE=wayland`，合成器为 niri），没有 X11 会话。
  egui / winit 的行为验证必须在 Wayland 下进行，不依赖 X11 专属路径。
- 交互 shell：fish。脚本不依赖交互 shell 配置：显式写 shebang
  （`#!/usr/bin/env bash` 或 `#!/usr/bin/env fish`），并保证在非交互环境下可运行。
- 辅助脚本只用于开发与验证，不进入产品运行时；项目不引入 Python / Node 运行时依赖。

## Windows（本机原生 MSVC 构建）

同一棵树也在这台 Windows 机器上用 MSVC 直接构建，不是交叉编译。Linux 侧因此不再
需要 `x86_64-pc-windows-gnu` target 与 `mingw-w64-gcc`；这些工具链可以卸载，仓库的
脚本与 CI 都不再引用它们。

- **MSVC 与 Windows SDK**：由 `msvc-kit` 安装在 `C:\data\msvc-kit-data`
  （MSVC `14.40.33807`、Windows SDK `10.0.19041.0`）。执行一次持久化后，它们被写进
  用户环境变量（`PATH`、`INCLUDE`、`LIB`、`CC`、`AR`、
  `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER`），普通 `cargo build` 与
  rust-analyzer 都直接可用：

  ```powershell
  msvc-kit setup --persistent   # 只需一次；之后重启编辑器与终端
  cargo build
  ```

  没有做持久化时，用 `msvc-kit run` 临时注入环境：

  ```bash
  msvc-kit run -- cargo build
  ```

  注意：Git for Windows 的 `usr\bin\link.exe` 仍可能排在 MSVC 之前，但持久化设置
  通过 `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER` 把链接器路径显式传给 rustc，
  因此不影响构建。

- **libmpv 导入库**：`mpv-engine` 需要 MSVC 导入库 `mpv.lib`，官方 Windows 构建只
  提供 `libmpv-2.dll`。用脚本生成（Windows 走 `dumpbin` + `lib`，Linux 走 LLVM 的
  `llvm-readobj` + `llvm-dlltool`）：

  ```bash
  ./scripts/mpv-import-lib.sh /path/to/libmpv-2.dll
  ```

  输出在 `dist/mpv-lib/`，构建时指向它：

  ```powershell
  $env:MPV_LIBRARY_DIR = "C:/project/rust/rossresscut/dist/mpv-lib"
  cargo build
  ```

- **打包**：从干净工作树运行 `./scripts/package-windows.sh`，产出
  `dist/rossresscut-<version>-windows-x64.zip` 与 `.sha256`。
- **崩溃日志**：未捕获的 panic 追加到 `%LOCALAPPDATA%\rossresscut\crash.log`
  （UTC 时间、版本与堆栈）；Linux 为 `$XDG_STATE_HOME/rossresscut/crash.log`
  （回退 `~/.local/state`）。
- **运行时 DLL**：`libmpv-2.dll` 必须与 exe 同目录或位于 `PATH`。集成测试的 exe 在
  `target\debug\deps` 下，需要把 DLL 也复制到那里。
- **FFmpeg / FFprobe**：`ffmpeg.exe`、`ffprobe.exe` 位于 `C:\bin` 并在 `PATH` 中。
- **准入门槛**：在 Git Bash 中一条命令跑完全部检查：

  ```bash
  MPV_LIBRARY_DIR=C:/data/mpv-msvc bash scripts/check.sh
  ```

  未做持久化环境时在前面加 `msvc-kit run --`。

- 程序以 `windows_subsystem = "windows"` 构建，自身没有控制台。所以所有外部
  `ffmpeg` / `ffprobe` 进程必须经 `media::process::command` 创建；它在 Windows 上
  设置 `CREATE_NO_WINDOW`。直接用 `std::process::Command` 会为每次探测、波形或解码
  另开一个控制台窗口。启动失败会弹原生错误对话框，界面规则见 `docs/ui-guidelines.md`。
- **不要把 Windows SDK 的 `bin\<version>\x64` 放进 `PATH`。** 该目录自带旧版
  `dxcompiler.dll` / `dxil.dll`，wgpu 的 D3D12 后端会从 `PATH` 加载它们，然后
  `DxcCreateInstance` 返回 `E_NOINTERFACE`，整个初始化失败：

  ```text
  DxcCreateInstance failed: 不支持此接口 (0x80004002)
  Failed to create surface for any enabled backend: {}
  ```

  由于程序是 GUI 子系统且没有控制台，双击时表现为**默默退出、没有任何提示**。
  `msvc-kit setup --persistent` 会把该目录写进用户 `PATH`，需要手动移除；
  `rc.exe` / `mt.exe` 只在编译 Windows 资源脚本时才需要，rustc 不使用。构建只需要
  MSVC 的 `Tools\MSVC\<version>\bin\Hostx64\x64` 在 `PATH` 上。
- 交互 shell 是 nushell，PowerShell 5.1 保留可用；脚本仍显式写 shebang，不依赖
  交互 shell 配置。

## 权限与包查询

- 本机 agent 运行环境的 `NoNewPrivs: 1`，且 **CapEff / CapPrm / CapBnd 全为 0**：
  `sudo`、`pkexec`、`su` 都无法获得 root。任何 `sudo` 尝试都只会得到
  `no new privileges` 拒绝，不是配置遗漏。
- 本机安装了 **`pacfiles` 0.3.0-1**（来自 `archlinuxcn` 仓库，依赖 `plocate`），是
  `pacman -F` 的快速替代，查询范围覆盖仓库而非仅已安装包：

  ```bash
  pacfiles '*/ffprobe'            # 文件 → 包，支持 glob
  pacfiles -q usr/bin/ffprobe     # 只输出包名
  pacfiles -l ffmpeg              # 包 → 文件
  pacfiles -x '.*/libav(codec|format)\.so'   # POSIX 扩展正则
  ```

  它只读，不改系统；数据库陈旧时由用户在宿主终端执行 `sudo pacman -Fy`（写入
  `/var/lib/pacman`）。例如 `usr/bin/ffprobe` 属于 `extra/ffmpeg`，不需要单独安装。
- 本机 `pacman -F` 会报第三方仓库（`arch4edu`）数据库不一致；文件查询优先用 `pacfiles`，
  同时把该告警报告给用户，不要静默忽略。
- 换机器或未安装 `pacfiles` 时回退到 `pacman -Fq usr/bin/ffprobe` 与 `pacman -Ql <pkg>`。
- 这些查询只用于判断是否需要装包；安装仍由用户手动执行，规则见
  `docs/development.md`。
