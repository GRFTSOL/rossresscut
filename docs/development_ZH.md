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

## 构建输入

- `assets/img/icon.png` 是窗口图标源图，`assets/img/icon.ico` 是 Windows 的多尺寸
  可执行文件图标；`build.rs` 读取两者，删除或改名会让构建失败。
- `build.rs` 的依赖只用于构建：`image`（把 PNG 缩放到窗口图标尺寸）与
  Windows 上的 `winresource`（嵌入 ico 与版本字符串）。两者都不进入运行时。
- 窗口图标尺寸由 `build.rs` 的 `WINDOW_ICON_SIZE` 决定，`src/ui/mod.rs` 从
  `OUT_DIR/window-icon.rgba` 读取；改动尺寸时两处必须同步。

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
  powershell -NoProfile -Command 'New-Item -ItemType Junction -Path "<fuck-u-code>\node_modules\tree-sitter-wasms" -Target "<tree-sitter-wasms>"'
  ```

  验证：再跑一次，输出中不再出现 `Tree-sitter init failed for rust`（`.sh` 脚本
  降级到 regex parser 不影响 Rust 结果）。

## 开发环境

当前开发机的工具链、路径与已知坑单独记录在 [`docs/local-environment.md`](local-environment_ZH.md)：
那部分只描述本机（Arch + Wayland、Windows MSVC via `msvc-kit`、`C:\bin` 等），
不代表产品支持范围；支持平台见 `GOALS.md` 与 `PLANS.md`。

通用的构建输入与准入门槛见上文。两个平台的打包脚本：

- Linux：`./scripts/package-linux.sh`
- Windows：`./scripts/package-windows.sh`（Git Bash；需要 MSVC 工具链与 `mpv.lib`）

Windows 运行期需要 `libmpv-2.dll` 与 exe 同目录，外部 `ffmpeg` / `ffprobe` 必须在
`PATH` 或 exe 同目录。

## 权限边界

agent 无法提权：当前运行环境设置了 `NoNewPrivs: 1`，`sudo` / `pkexec` / `su` 都无法获得
root。这是运行环境的既定限制，不是配置遗漏，也不允许尝试绕过。
**系统级安装一律由用户手动执行。**

agent 的职责：

1. 只读检测：`command -v ffmpeg ffprobe`、`pacman -Q ffmpeg`。
2. 缺失时报告，并给出可直接复制的安装命令（例如 `sudo pacman -S --needed ffmpeg`）。
3. 用户确认完成后复检版本与路径，再继续后续工作。

禁止：

- 尝试 `sudo`、`pkexec`、`su -c`、`sudo -S` 读密码、`sudo -A` 配合自建 askpass。
- 修改 `/etc/sudoers`、`/etc/sudoers.d/*`、`/etc/sudo.conf`，或建议为用户机器配置 `NOPASSWD`。
- 在用户未明确要求时执行 `pacman -Syu`、`pacman -R*`、`systemctl` 修改系统服务。
- 把任何密码写入脚本、环境变量、日志或提交到仓库。

若某个 crate 需要系统开发库，同样报告包名、由用户安装，agent 只做检测。项目自身的
Rust 依赖与项目内脚本不得要求 root。

报告“缺依赖”之前，先用 `pacman -Fq usr/bin/ffprobe`（文件 → 包）或 `pacman -Ql <pkg>`
（包 → 文件）确认归属；本机更快的替代见 `docs/local-environment.md`。查询只用于决定是否
装包，安装仍走上述手动流程。

---

## UI 检查（AT-SPI，仅 Linux）

`scripts/ui_a11y.py` 通过 AT-SPI 读取或点击正在运行的 `rossresscut` 窗口。
Windows 没有 AT-SPI，只能用截图配合 `keybd_event` / 鼠标事件注入，例如
`target/` 下的一次性 PowerShell 脚本。
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

不自动 commit、push、reset、rebase 或修改历史，除非用户明确要求（同
`AGENTS.md`）。保持修改范围清晰。

不要把以下内容混在一个修改中：

- 功能
- 大规模重构
- 格式化
- 依赖升级
- 无关 bug 修复

---

## 红线清单

必须遵守 `AGENTS.md` 的硬约束（原生 Rust、无 `unsafe`、整数时间、UI 帧内不阻塞、
只经 Application 命令修改状态、禁止 shell 拼接、禁止静默重编码、禁止 `unwrap`
处理外部输入）。开发流程上额外视为未完成的行为：

- ❌ 提交带 warning 或 `cargo fmt --check` 不通过的代码。
- ❌ 未经实际运行就声称“已经测试通过”。
- ❌ 用 `fuck-u-code` 的分数替代实际测试。

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
