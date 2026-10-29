# 发布流程

发布以 commit 为单位；不要从未提交的工作区构建。

## 1. 发布前检查

```bash
git status --short
./scripts/check.sh
```

- 工作区只允许出现明确说明的未跟踪产物（例如 `dist/`）。
- 确认 `Cargo.toml` 的版本与 `src/lib.rs` 的 `PHASE` 正确。

## 2. 构建

```bash
cargo build --release
./target/release/rossresscut
```

至少验证窗口能启动、字体扫描正常、能打开一个本地媒体文件。

## 3. 打包（Linux）

```bash
mkdir -p dist
SHA=$(git rev-parse --short HEAD)
tar -C target/release \
  -czf "dist/rossresscut-0.1.0-phase5-$SHA-x86_64-linux.tar.gz" \
  rossresscut
```

产物：

```text
dist/rossresscut-<version>-<phase>-<commit>-x86_64-linux.tar.gz
```

`dist/` 不提交到 Git。

## 4. 运行时依赖

发布包不包含以下组件：

- `libmpv.so.2`（系统 `mpv`）
- `ffmpeg` / `ffprobe`
- XDG desktop portal
- EGL / GBM / GL

发布说明必须写明这些依赖。

## 5. Windows / macOS

尚未发布，阻塞项记录在 `PLANS.md` 的 Phase 8。

Windows 至少需要：

- Windows 文件打开 / 保存对话框（当前 `rfd` 仅作为 Linux 依赖）
- `libmpv-2.dll` 与 import library，随包分发
- Windows 版 FFmpeg / FFprobe
- `x86_64-pc-windows-gnu` 交叉编译或 Windows 原生 MSVC 构建
- D3D11VA 预览验证

macOS 至少需要 libmpv dylib、VideoToolbox 验证、签名与公证。

## 6. 对外发布

未经用户明确同意，不得：

- 创建 GitHub Release 或上传产物
- 提交到软件商店
- 发布公告或更新网站

发布前提供准确的版本号、commit、变更摘要与已知限制。
