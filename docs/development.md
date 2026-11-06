# 开发与工具链

本文件记录本机开发环境、命令、权限边界与提交规则。贡献流程见 `CONTRIBUTING.md`。

## 准入门槛

Cargo 工程建立之后，以下命令是本地与 CI 的共同准入门槛：

```bash
cargo build
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
cargo fmt --all --check
```

## 规则

- 提交前上述命令必须全部通过。
- `--no-default-features` 的 clippy 用于验证无 `mpv-preview` 时的外部 FFmpeg 回退路径仍可编译；它不运行需要 libmpv 的测试。
- 编译警告零容忍：`unused import`、`dead_code`、`deprecated API` 都必须为零。
- 格式问题不靠人工审查，交给 `cargo fmt`；`cargo fmt --all --check` 失败即视为未完成。
- 工具链版本由 `rust-toolchain.toml` 固定，避免不同机器行为差异；升级工具链属于独立变更。
- 命令块中的参数与 `clippy` 的 `-D warnings` 不得为了绕过报错而删减。
- `fuck-u-code analyze` 的 `Overall Score`（及 JSON 里的 `score`）**越高越好**；文本报告中的 `Issue Score` 是反向值（`100 - score`），越高越差；`Metrics Details` 里的百分比是问题占比，越低越好。
- `fuck-u-code` 的函数级指标依赖 `tree-sitter-wasms` 的 WASM 语法文件。pnpm 全局安装不会把它链接进 `fuck-u-code/node_modules`，缺失时工具会**静默降级到 regex parser**，把整个 `impl` 块当成一个函数，函数长度与复杂度全部失真。

  判断方法：输出里出现 `Tree-sitter init failed for rust, falling back to regex parser` 就说明数据不可信；此时以 `cargo clippy -W clippy::too_many_lines` 为准。

  修复（重装后可能复发）：

  ```bash
  PNPM_GLOBAL=$(pnpm root -g)
  for dir in "$PNPM_GLOBAL"/.pnpm/fuck-u-code@*/node_modules/fuck-u-code/node_modules; do
    ln -s "$PNPM_GLOBAL"/.pnpm/tree-sitter-wasms@*/node_modules/tree-sitter-wasms "$dir/tree-sitter-wasms"
  done
  ```

  Windows 上 `ln -s` 不可靠（需要开发者模式或管理员），改用目录联接；两侧路径
  按实际的 pnpm store 替换：

  ```powershell
  powershell -NoProfile -Command 'New-Item -ItemType Junction -Path "<eff-u-code>\node_modules\tree-sitter-wasms" -Target "<tree-sitter-wasms>"'
  ```

  验证：再跑一次，输出中不再出现 `Tree-sitter init failed for rust`（`.sh` 脚本
  降级到 regex parser 不影响 Rust 结果）。

## 开发环境

本节只记录**会改变构建与验证行为**的机器约定，不是产品支持范围。

- **操作系统**：Arch Linux（滚动发行版）。系统包用 `pacman` 安装，例如 `sudo pacman -S ffmpeg`；不通过脚本直接下载系统级二进制。
- **Rust 工具链**：由 `rustup` 管理，版本由 `rust-toolchain.toml` 固定（当前 `1.99.1`，含 `rustfmt` 与 `clippy`）。缺组件时用 `rustup component add rustfmt clippy`。
- **工具链升级**：升级 `rust-toolchain.toml` 是独立变更，不与功能改动混在一起。
- **FFmpeg / FFprobe**：使用系统包（`/usr/sbin/ffmpeg`、`/usr/sbin/ffprobe`；`ffprobe` 由 `ffmpeg` 包提供，不是独立包），项目不自带二进制。滚动发行版会改变实际版本，因此测试不得假设固定版本行为。
- **桌面会话**：Wayland（`XDG_SESSION_TYPE=wayland`，合成器为 niri），没有 X11 会话。egui / winit 的行为验证必须在 Wayland 下进行，不依赖 X11 专属路径。
- **交互 shell**：fish。脚本不依赖交互 shell 配置：显式写 shebang（`#!/usr/bin/env bash` 或 `#!/usr/bin/env fish`），并保证在非交互环境下可运行。
- **辅助脚本**：只用于开发与验证，不进入产品运行时；项目不引入 Python / Node 运行时依赖。

### Windows（本机原生 MSVC 构建）

同一棵树也在这台 Windows 机器上用 MSVC 直接构建，不是交叉编译。

- **MSVC 与 Windows SDK**：由 `msvc-kit` 安装在 `C:\data\msvc-kit-data`
  （MSVC `14.40.33807`、Windows SDK `10.0.19041.0`）。它们不在默认 `PATH` 上，
  且 Git for Windows 的 `C:\application\git\usr\bin\link.exe` 会遮蔽 MSVC 的
  `link.exe`，直接 `cargo build` 会在链接阶段失败。所有 cargo 命令经
  `msvc-kit` 运行：

  ```bash
  msvc-kit run -- cargo build
  ```

- **libmpv 导入库**：`mpv-engine` 需要 MSVC 导入库 `mpv.lib`，官方 Windows
  构建只提供 `libmpv-2.dll`。本机在 `C:\data\mpv-msvc\` 下由 `dumpbin /exports`
  生成 `.def`，再 `lib /def:mpv.def /machine:x64` 生成 `mpv.lib`；构建时用
  `MPV_LIBRARY_DIR` 指向该目录：

  ```bash
  MPV_LIBRARY_DIR=C:/data/mpv-msvc msvc-kit run -- cargo build
  ```

- **运行时 DLL**：`libmpv-2.dll` 必须与 exe 同目录或位于 `PATH`。集成测试的
  exe 在 `target\debug\deps` 下，需要把 DLL 也复制到那里。
- **FFmpeg / FFprobe**：`ffmpeg.exe`、`ffprobe.exe` 位于 `C:\bin` 并在 `PATH` 中。
- **准入门槛**：在 Git Bash 中一条命令跑完全部检查：

  ```bash
  MPV_LIBRARY_DIR=C:/data/mpv-msvc msvc-kit run -- bash scripts/check.sh
  ```

- 程序以 `windows_subsystem = "windows"` 构建，自身没有控制台。所以所有外部
  `ffmpeg` / `ffprobe` 进程必须经 `media::process::command` 创建；它在 Windows
  上设置 `CREATE_NO_WINDOW`。直接用 `std::process::Command` 会为每次探测、
  波形或解码另开一个控制台窗口。
- 交互 shell 是 nushell，PowerShell 5.1 保留可用；脚本仍显式写 shebang，
  不依赖交互 shell 配置。

产品支持平台仍以 `GOALS.md` 与 `PLANS.md` 中的 Linux / Windows / macOS 为准。本节描述的是当前开发机的验证环境，不能作为"只支持 Arch Linux"的依据。

## 系统包与权限边界

### 硬约束：agent 无法提权

当前 agent 运行环境设置了 `NoNewPrivs: 1`，且 **CapEff / CapPrm / CapBnd 全为 0**。
`sudo`、`pkexec`、`su` 都无法获得 root：

```text
sudo: The "no new privileges" flag is set, which prevents sudo from running as root.
```

这是运行环境的既定限制，不是配置遗漏，也不允许尝试绕过。

**结论：系统级安装一律由用户手动执行。**

### agent 的职责

1. 只读检测：

   ```bash
   command -v ffmpeg ffprobe
   pacman -Q ffmpeg
   ```

2. 缺失时报告，并给出可直接复制的命令：

   ```bash
   sudo pacman -S --needed ffmpeg
   ```

3. 用户确认完成后复检版本与路径，再继续后续工作。

### 禁止

- 尝试 `sudo`、`pkexec`、`su -c`、`sudo -S` 读密码、`sudo -A` 配合自建 askpass。
- 修改 `/etc/sudoers`、`/etc/sudoers.d/*`、`/etc/sudo.conf`，或建议为用户机器配置 `NOPASSWD` 以便 agent 提权。
- 在用户未明确要求时执行 `pacman -Syu`、`pacman -R*`、`systemctl` 修改系统服务。
- 把任何密码写入脚本、环境变量、日志或提交到仓库。

### 需要 root 的构建依赖

如果某个 crate 需要系统开发库，同样按上述流程：报告包名，由用户安装，agent 只做检测。

项目自身的 Rust 依赖与项目内脚本不得要求 root。

### 包与文件归属查询

本机安装了 **`pacfiles` 0.3.0-1**（来自 `archlinuxcn` 仓库，依赖 `plocate`），
它是 `pacman -F` 的快速替代，查询范围覆盖仓库，不限于已安装包。

```bash
# 文件 → 包，支持 glob
pacfiles '*/ffprobe'
pacfiles 'usr/bin/ffprobe'

# 只输出包名，便于脚本消费
pacfiles -q usr/bin/ffprobe

# 包 → 文件
pacfiles -l ffmpeg

# POSIX 扩展正则
pacfiles -x '.*/libav(codec|format)\.so'
```

**用途**：在报告"缺依赖"之前，先确认某个命令、头文件或共享库由哪个包提供，
以及现有包是否已经包含它。例如 `usr/bin/ffprobe` 属于 `extra/ffmpeg`，不需要单独安装。

**边界**：

- `pacfiles` 只读查询，可以作为默认排查工具；它不安装、不修改系统。
- 数据库陈旧时，刷新需要写 `/var/lib/pacman`，由用户在宿主终端执行 `sudo pacman -Fy`。
- 换机器或未安装 `pacfiles` 时回退：

  ```bash
  pacman -Ql <pkg>             # 包 → 文件，仅限已安装包
  pacman -Fq usr/bin/ffprobe   # 文件 → 包，需要 pacman 文件数据库
  ```

- 本机 `pacman -F` 会报第三方仓库（`arch4edu`）数据库不一致；文件查询优先用 `pacfiles`，
  同时把该告警报告给用户，不要静默忽略。
- 查询只用于决定"是否需要装包"，**安装仍然走本节开头的手动流程**。

---

## UI 检查（AT-SPI）

`scripts/ui_a11y.py` 通过 AT-SPI 读取或点击正在运行的 `rossresscut` 窗口。
它用于开发与代理验证，不进入产品运行时。

前提：

- 应用使用默认的 `accesskit` feature 构建。
- 本机有 PyGObject 的 Atspi 绑定（Arch：`python-gobject`、`at-spi2-core`）。
- niri 默认不开启 AT-SPI；脚本的 `--enable` 会设置运行时的
  `org.a11y.Status.IsEnabled`。

```bash
# 先启动应用
./target/release/rossresscut [media-file] &

./scripts/ui_a11y.py --enable
./scripts/ui_a11y.py --status
./scripts/ui_a11y.py --dump
./scripts/ui_a11y.py --dump --role button
./scripts/ui_a11y.py --dump --role button --rects
./scripts/ui_a11y.py --click "⚙" --role button
./scripts/ui_a11y.py --click "文件" --role button
./scripts/ui_a11y.py --disable
```

说明：

- `--role` 只控制打印或点击匹配；树仍会完整遍历。
- `--contains` 把精确名称匹配改为子串匹配；`--index` 选择同名控件中的第几个。
- `--rects` 输出屏幕坐标与尺寸，可用于粗略布局检查。
- 自绘的时间轴与视频预览在 AT-SPI 中通常只是 `unknown`，没有按钮角色或
  action；拖动、滚轮和视频画面仍需截图或输入注入。
- 颜色、字体渲染与布局细节仍必须用截图验证。
- 脚本会过滤 AccessKit 与 pyatspi 之间已知的无害 AT-SPI 警告。

### 导出 UI 冒烟

`scripts/ui_export_smoke.py` 用 AT-SPI 驱动完整导出流程，不需要人工点击：

```bash
./scripts/ui_export_smoke.py
./scripts/ui_export_smoke.py --keep
./scripts/ui_export_smoke.py --binary target/debug/rossresscut
```

- 在 headless `cage` 中启动应用，生成一个带 2 个章节的 1080p30 素材；章节自动生成 Segment。
- 配置 `smart_cut = true`，让两次导出都有可观察的运行时间。
- 第一次导出断言运行中、完成状态、输出时长与 2 个章节；第二次在运行中点击取消，
  断言「Export cancelled」且没有留下输出文件。
- 要求本机没有其他 `rossresscut` 实例；脚本结束时会关闭它启动的 cage 进程组。
- 失败时打印当前的 label/button 列表与 cage 日志尾部。

## 发布打包（Linux）

`scripts/package-linux.sh` 构建 release 二进制并生成 pacman 包：

```bash
./scripts/package-linux.sh
```

- 要求工作树干净；脚本会检查 `Cargo.toml` 与 `packaging/PKGBUILD` 的版本一致。
- 依次执行 `./scripts/check.sh`、`cargo build --release --locked`、`makepkg`。
- 产物在 `dist/`：`rossresscut-<version>-<pkgrel>-x86_64.pkg.tar.zst`；
  包内容、运行时依赖与许可证说明见 `RELEASING.md`。
- `packaging/PKGBUILD` 复用当前工作树的 `target/release`，不在 `build()` 里调用
  cargo：makepkg 每次把 `SOURCE_DATE_EPOCH` 设为当前时间，该值属于 Cargo 指纹，
  会让整个依赖图重新编译并冲掉本地 release 缓存。

## Git 与提交

不要自动：

- commit
- push
- reset
- rebase
- 修改历史

除非用户明确要求。

保持修改范围清晰。

不要把以下内容混在一个修改中：

- 功能
- 大规模重构
- 格式化
- 依赖升级
- 无关 bug 修复

---

## 红线清单

以下行为直接视为未完成：

- ❌ 引入 `Electron` / `Tauri` / `WebView` / JS UI 框架 / `Node.js` 运行时。
- ❌ 在 egui 帧回调中同步执行 FFmpeg、FFprobe、解码或大量文件 I/O。
- ❌ 用 `f32` / `f64` 秒作为媒体时间的唯一表示。
- ❌ 在项目代码中使用 `unsafe`、FFI、裸指针、`transmute`、`union`、`MaybeUninit`、`ManuallyDrop` 或 `static mut`。
- ❌ 让 `mpv-engine` / `wgpu` 的裸指针、FFI 类型或底层句柄越过 `media::preview` 与 UI 渲染适配层。
- ❌ 同时让 mpv 与 cpal 输出音频。
- ❌ 静默把 `stream copy` 降级为重新编码，或把重编码结果标记为无损。
- ❌ 通过 shell 拼接命令执行 FFmpeg。
- ❌ 用 `unwrap()` / `expect()` 处理用户输入或外部进程输出。
- ❌ 让 `serde_json::Value` 越过 FFprobe 解析边界继续传播。
- ❌ 在命令回调之外直接修改领域状态。
- ❌ 提交带 warning 或 `cargo fmt --check` 不通过的代码。
- ❌ 未经实际运行就声称"已经测试通过"。

---

## 完成标准

一个功能只有满足以下条件才算完成：

- [ ] 编译通过
- [ ] 相关测试通过
- [ ] 新功能有合理测试
- [ ] 错误路径经过考虑
- [ ] UI 不会被长任务阻塞
- [ ] 没有无必要的新依赖
- [ ] 新增依赖的理由已记录
- [ ] 公开 API 有 `///` 文档注释
- [ ] `cargo fmt --all --check` 通过
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` 通过
- [ ] 项目代码无 `unsafe`；外部 GPU 句柄只在 `media::preview` 与 UI 渲染适配层之间传递
- [ ] 相关文档已经更新
- [ ] 没有产生无关修改
- [ ] 功能、重构、格式化、依赖升级没有混在同一次修改中
- [ ] 实际验证了相关功能

如果没有实际测试：

**禁止声称“已经测试通过”。**

如果只是理论上 FFmpeg 支持：

**禁止声称“项目已经支持该格式”。**
