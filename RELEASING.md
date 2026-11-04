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
sudo pacman -U dist/rossresscut-0.3.0-1-x86_64.pkg.tar.zst
sudo pacman -R rossresscut
```

包内容：

```text
/usr/bin/rossresscut
/usr/share/applications/rossresscut.desktop
/usr/share/icons/hicolor/scalable/apps/rossresscut.svg
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

- 项目目前没有 LICENSE，`ATTRIBUTION.md` 明确记录「尚未确定最终许可证」。
  包的 `license=('custom')` 只是占位，不代表许可证已确定。
- 二进制动态链接 `libmpv.so.2`（`mpv` 包为 GPL-2.0-or-later）。
  公开分发前必须先确定本项目许可证，并确认 GPL 合规方式
  （提供对应源码 / 链接声明等）。
- 在以上问题解决前，只做本机或私有分发。

## 6. Windows / macOS

尚未发布，阻塞项记录在 `PLANS.md` 的 Phase 8。

Windows 至少需要：

- Windows 文件打开 / 保存对话框（当前 `rfd` 仅作为 Linux 依赖）
- `libmpv-2.dll` 与 import library，随包分发
- Windows 版 FFmpeg / FFprobe
- `x86_64-pc-windows-gnu` 交叉编译或 Windows 原生 MSVC 构建
- D3D11VA 预览验证

macOS 至少需要 libmpv dylib、VideoToolbox 验证、签名与公证。

## 7. 对外发布

未经用户明确同意，不得：

- 创建 GitHub Release 或上传产物
- 提交到软件商店
- 发布公告或更新网站

发布前提供准确的版本号、commit、变更摘要与已知限制。
