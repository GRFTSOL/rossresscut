# 发布流程

发布以 commit 为单位；不要从未提交的工作区构建。

## 1. 发布前检查

```bash
git status --short
./scripts/check.sh
```

- 工作区只允许出现明确说明的未跟踪产物（例如 `dist/`）。
- 确认 `Cargo.toml` 的版本与 `packaging/PKGBUILD` 的 `pkgver` 一致；
  `scripts/package-linux.sh` 会检查并在不一致时退出。
- 更新 `PLANS.md` 中当前 Phase 的完成项，记录已知限制。
- 在带 Vulkan + DMA-BUF 的 Linux 实机上运行 GPU 回归
  （CI runner 没有 DRM render node，不能覆盖）：

  ```bash
  cargo test --release --features mpv-preview --test mpv_gpu_regression -- --ignored
  cargo test --release --features mpv-preview --test mpv_preview -- --ignored
  ```

  其中 `resizing_the_export_updates_the_texture` 覆盖 mpv 导出尺寸变化后的
  纹理尺寸与颜色。
- 打开一个大于窗口的媒体，拖动窗口边改变大小：确认预览比例不变、没有被放大，
  日志出现新的 `mpv preview size WxH`，且连续拖动时不会每帧重配 export。

## 2. 构建

```bash
cargo build --release --locked
./target/release/rossresscut
```

至少验证窗口能启动、字体扫描正常、能打开一个本地媒体文件。
`build.rs` 在构建时读取 `git rev-parse --short HEAD` 注入「关于」窗口；
因此必须从 Git 工作树构建，而不是从解包后的源码目录构建。

## 3. 打包（Linux / pacman）

```bash
./scripts/package-linux.sh
```

脚本依次执行：干净工作树检查 → 版本一致性检查 → `./scripts/check.sh`
→ `cargo build --release --locked` → `makepkg`。

产物：

```text
dist/rossresscut-<version>-<pkgrel>-x86_64.pkg.tar.zst
```

安装与卸载：

```bash
sudo pacman -U dist/rossresscut-0.4.0-1-x86_64.pkg.tar.zst
sudo pacman -R rossresscut
```

包内容：

```text
/usr/bin/rossresscut
/usr/share/applications/rossresscut.desktop
/usr/share/icons/hicolor/scalable/apps/rossresscut.svg
/usr/share/licenses/rossresscut/LICENSE
/usr/share/licenses/rossresscut/ATTRIBUTION.md
```

`packaging/PKGBUILD` 直接在当前工作树的 `target/release/` 上构建，因此
复用本地构建缓存，版本、commit 与产物一致。这条路线只适合本地/私有发布；
如果要发布到 AUR 或公开仓库，应改为 `source=()` 之外的 tagged tarball
流程，并补上真实的 `url` 与校验和。

`dist/` 与 `packaging/{src,pkg}` 不提交到 Git。

## 4. 运行时依赖

包声明了以下 `depends`：

- `mpv`：`libmpv.so.2`，预览与播放
- `ffmpeg`：`ffmpeg` / `ffprobe`，探测、导出、波形、场景检测
- `xdg-desktop-portal`：文件打开 / 保存对话框（`rfd` 的 xdg-portal 后端）
- `mesa` / `libglvnd` / `wayland` / `libxkbcommon`：wgpu / winit / EGL
- `alsa-lib`：音频输出
- `glibc` / `gcc-libs`

`optdepends`：

- `xdg-desktop-portal-gtk`：合成器没有原生 portal 实现时的文件选择后端
- `libx11`：X11 会话

发布说明必须写明这些依赖，以及硬件解码由 mpv 的 `hwdec` 负责。

## 5. 许可证与再分发

- 项目以 **GPL-3.0-or-later** 发布，`LICENSE` 为 GPLv3 全文；
  `Cargo.toml` 与 `packaging/PKGBUILD` 的 license 字段必须与之一致。
- 二进制动态链接 `libmpv.so.2`（`mpv` 包为 GPL-2.0-or-later）。
  GPL-3.0-or-later 与它兼容；`cpal` 的 Apache-2.0 也与 GPLv3 兼容。
- 分发二进制时必须同时提供对应源码（或书面提供方式），并保留
  `LICENSE` 与 `ATTRIBUTION.md`。
- 不再使用 `license=('custom')`；决策见 `docs/decisions/0015-project-license.md`。

## 6. Windows / macOS

Windows 已能原生构建并产出便携包；macOS 尚未开始。剩余项见 `PLANS.md`。

### Windows

构建（原生 MSVC，当前主路径）：

- 工具链：`msvc-kit`（便携 MSVC + Windows SDK）。执行一次
  `msvc-kit setup --persistent` 后普通 `cargo build` 即可；PATH 注意事项与
  `mpv.lib` 生成步骤见 `docs/development.md`。

文件对话框：

- `rfd` 0.17 的 Windows 后端使用 Shell COM 的 `IFileOpenDialog` /
  `IFileSaveDialog`（`IFileDialog` 派生接口），不需要自建对话框。
- `Cargo.toml` 在 `[target.'cfg(target_os = "windows")'.dependencies]` 下启用
  `rfd = { version = "0.17", default-features = false }`，
  `FILE_DIALOG_SUPPORTED` 覆盖 Linux 与 Windows。
- 尚未人工验收对话框行为（应用本身已在实机运行）。

链接与运行时文件：

- `rsmpv-sys` 在编译期用 `-lmpv` 链接，MSVC 需要导入库 `mpv.lib`。官方 Windows
  构建只提供 `libmpv-2.dll`，因此用 `dumpbin /exports` 生成 `.def`，再
  `lib /def:mpv.def /machine:x64` 生成，所在目录通过 `MPV_LIBRARY_DIR` 传入。
- 运行期需要 `libmpv-2.dll`。Windows 默认 DLL 搜索顺序先找可执行文件目录，
  放在 exe 同目录即可（`PATH` 亦可）。
- `ffmpeg.exe` / `ffprobe.exe` 放在 exe 同目录或 `PATH`；两者都由
  `media::process::command` 启动，Windows 上带 `CREATE_NO_WINDOW`。

便携包：

- 产物：`dist/rossresscut-<version>-windows-x64.zip`，解压即可运行。
- 内含 `rossresscut.exe`、`libmpv-2.dll`、`ffmpeg.exe`、`ffprobe.exe`、
  `LICENSE`、`ATTRIBUTION.md` 与 `README-WINDOWS.txt`；包内说明记录了运行方式、
  日志与故障排查。
- 打包目前是手工步骤，尚未脚本化（见 `docs/open-questions.md` 的 OQ-28）。

验证状态（0.4.0）：

- [x] 原生 MSVC 构建、`cargo test`、两轮 `cargo clippy` 通过。
- [x] 预览（wgpu DX12 + libmpv 导出纹理）、波形、关键帧索引，NVIDIA `nvdec`。
- [x] 中文名、带空格路径的媒体可正常打开、预览并生成波形。
- [x] 启动失败弹原生错误对话框（GUI 无控制台）。
- [ ] 文件打开 / 保存对话框、拖放、高 DPI 的人工验收。
- [ ] 导出路径选择、章节写入与多段 concat 的人工验收（自动化测试已通过）。

历史方案（已不再维护）：在 Arch Linux 上用 mingw 交叉编译到
`x86_64-pc-windows-gnu`。0.3.0 的测试包由该方式构建；0.4.0 起以原生 MSVC 为准，
命令保留在这里仅作参考：

```bash
sudo pacman -S --needed mingw-w64-gcc
rustup target add x86_64-pc-windows-gnu
MPV_LIBRARY_DIR=/path/to/libmpv-import-lib \
  cargo build --release --target x86_64-pc-windows-gnu
```

- 该方式需要 `libmpv.dll.a`（mpv 官方 `mpv-dev-*.7z`），且未在实机验证。

macOS 至少需要 libmpv dylib、VideoToolbox 验证、签名与公证。

## 7. 对外发布

未经用户明确同意，不得：

- 创建 GitHub Release 或上传产物
- 提交到软件商店
- 发布公告或更新网站

发布前提供准确的版本号、commit、变更摘要与已知限制。
