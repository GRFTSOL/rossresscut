# 开发计划

## 当前阶段

**Phase 8：Linux 发布（进行中）**

- Linux：release 构建、`packaging/PKGBUILD` 与 pacman 包已完成（见 Phase 8）。
- Windows：原生 MSVC 构建、测试与 clippy 已打通（进行中）；macOS 未开始。
- Phase 0–7C 已完成；详细实现与验证记录归档在 `docs/history/completed-phases.md`。
- 未解决问题记录在 `docs/open-questions.md`。

## 已完成阶段摘要

| 阶段 | 结果 | 关键约束 / 决策 |
| --- | --- | --- |
| Phase 0 | 项目基础 | Rust + egui；`unsafe_code = "forbid"` |
| Phase 1 | 媒体分析 | FFprobe JSON 边界；整数时间类型 |
| Phase 2 | 媒体预览 | libmpv 主路径（ADR 0010）；外部 FFmpeg 回退 |
| Phase 3 | Timeline | 有序 EDL、稳定 `SegmentId`、允许重叠（ADR 0008） |
| Phase 4 | 播放链路与无损导出 | 4A 测量；4B/4C mpv 预览；4D 单段 stream copy |
| Phase 5 | 多片段合并 | 临时片段 + concat demuxer；顺序只来自 Timeline |
| Phase 6 | 波形 | 外部 FFmpeg 解码 peak；时间轴背景绘制（ADR 0011） |
| Phase 7 | 高级功能 | 项目文件（ADR 0012）、批处理/合并、字幕/流选择、smart cut（ADR 0013/0014） |

Phase 5 的导出语义、Phase 6 的波形设计、Phase 7 的已知限制等细节见
`docs/history/completed-phases.md`。

---

# 参考实现研究

LosslessCut 是产品行为、媒体语义和 FFmpeg 工作流的参考实现，不是本项目的架构或代码模板。

- 研究顺序：本项目代码 / ADR → FFmpeg 官方文档 → LosslessCut 实现与 issue / commit → 实验验证 → 设计本项目实现。
- 重点：FFmpeg 导出参数、`-ss` / `-to` / `-t`、stream copy、PTS/DTS/time base、keyframe、smart cut、concat、音画同步、容器兼容性、metadata、字幕、chapter。
- LosslessCut 使用 GPL-2.0：只阅读、理解和验证行为；不复制源代码，不做逐行 TypeScript → Rust 翻译。
- 阶段约束：普通无损导出只做 stream copy；smart cut 已在 Phase 7C 实现，默认关闭，重编码范围与告知要求见 ADR 0013 / 0014。

---

# Phase 8：发布

## Linux

- [x] Release build 已验证（`cargo build --release`）
- [x] 发布包说明运行时依赖：`libmpv.so.2`、`ffmpeg` / `ffprobe`、XDG desktop portal、EGL / GBM
- [x] 打包格式：pacman 包（`packaging/PKGBUILD`，`./scripts/package-linux.sh`
  输出 `dist/rossresscut-<version>-<pkgrel>-x86_64.pkg.tar.zst`）
- [x] desktop entry 与 hicolor 图标随包安装
- [x] 已在 Arch / Wayland 上实际验证：`./scripts/package-linux.sh` 产出
  `rossresscut-0.3.0-1-x86_64.pkg.tar.zst`，包内二进制可启动，
  Wayland app_id 为 `rossresscut`，与 desktop entry 一致
- [x] 许可证：项目以 GPL-3.0-or-later 发布（ADR 0015）；`libmpv`（GPL-2.0-or-later）
  与 `cpal`（Apache-2.0）均与 GPLv3 兼容
- [ ] 公开仓库 / AUR：暂不考虑（个人自用）；将来需要时补 tagged tarball 源、
  `url` 与校验和

## Windows

- [x] Windows 文件打开 / 保存对话框：`rfd` 的 Windows 后端已接入
  （`IFileOpenDialog` / `IFileSaveDialog` COM 接口），`FILE_DIALOG_SUPPORTED`
  已覆盖 Windows
- [ ] 文件对话框、拖放与高 DPI 的实机验收
- [x] Windows 版 libmpv：已用 MSVC 导入库 `mpv.lib`（由 `libmpv-2.dll`
  的 `dumpbin /exports` + `lib /def:` 生成）链接成功；运行期 `libmpv-2.dll`
  放 exe 同目录
- [x] Windows 版 FFmpeg / FFprobe：`ffmpeg.exe` / `ffprobe.exe` 位于 `PATH`，
  探测、波形、场景检测与导出均已在 Windows 上跑通
- [x] 原生 MSVC 构建：`cargo build` / `cargo test` / 两轮 `clippy` 均通过；
  `msvc-kit setup --persistent` 后普通 `cargo build` 即可（`MPV_LIBRARY_DIR` 仍需）
- [x] 中文名与含空格路径的媒体实测：探测、预览、波形均正常
- [x] 启动失败可见：Windows 上弹原生错误对话框（`rfd` MessageDialog）
- [x] 硬件解码实测：预览走 `vo=libmpv` + OpenGL，`hwdec=auto-safe` 在 NVIDIA
  机器上解析为 `nvdec`（`d3d11va` 需要 `gpu-context=d3d11`，该路径不可用）；
  播放信息与日志 `mpv preview decoder:` 可见
- [x] 便携包：`./scripts/package-windows.sh` 产出
  `dist/rossresscut-<version>-windows-x64.zip`（exe + DLL + FFmpeg + 许可证 +
  README）与 `.sha256`，双击可运行
- [ ] 导出路径选择、章节写入与多段 concat 的人工验收（自动化测试已在
  Windows 通过，尚未由用户在界面上逐项确认）
- [x] Windows 包自动化：`scripts/mpv-import-lib.sh` 生成 `mpv.lib`，
  `scripts/package-windows.sh` 打 zip
- [x] 运行期失败可见性：panic 写入 `crash.log`（`logging::install_panic_hook`）
- [ ] 应用图标（窗口 / 任务栏）尚未设置
- [x] 交叉编译不再是主路径：Windows 版改为原生 MSVC 构建；mingw 交叉编译
  方案保留在 git 历史中，不再维护

## macOS

- [ ] libmpv dylib 分发与签名
- [ ] VideoToolbox 硬件解码验证
- [ ] 公证与安装包

## 通用

- [ ] 自动发布
- [ ] 安装包

---

# 开发原则

只处理：

> 当前阶段中最小的下一步。

不要一次实现整个 Phase。

每完成一个阶段：

1. 更新本文件。
2. 更新架构文档。
3. 更新测试。
4. 更新相关 ADR。
5. 确认 exit criteria。
