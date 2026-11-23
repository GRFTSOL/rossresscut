# 开发计划

## 当前阶段

**Phase 0–8 已全部完成。**

- 发布阶段（Phase 8）的收尾记录见下文；v0.4.2 起 Linux pacman 包由 GitHub Actions 在
  `v*` tag 上构建并附加到同名 Release；v0.4.3 起 Windows 便携包也由 Actions 构建。
- 详细实现与验证记录归档在 `docs/history/completed-phases_ZH.md`。
- 之后的工作是功能完善与缺陷修复，不再按 Phase 编号；涉及真实取舍时仍然写 ADR。
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
`docs/history/completed-phases_ZH.md`。

---

# 参考实现研究

LosslessCut 是产品行为、媒体语义和 FFmpeg 工作流的参考实现，不是本项目的架构或代码模板。

- 研究顺序：本项目代码 / ADR → FFmpeg 官方文档 → LosslessCut 实现与 issue / commit → 实验验证 → 设计本项目实现。
- 重点：FFmpeg 导出参数、`-ss` / `-to` / `-t`、stream copy、PTS/DTS/time base、keyframe、smart cut、concat、音画同步、容器兼容性、metadata、字幕、chapter。
- LosslessCut 使用 GPL-2.0：只阅读、理解和验证行为；不复制源代码，不做逐行 TypeScript → Rust 翻译。
- 阶段约束：普通无损导出只做 stream copy；smart cut 已在 Phase 7C 实现，默认关闭，重编码范围与告知要求见 ADR 0013 / 0014。

---

# Phase 8：发布（已完成）

- **Linux**：pacman 包（`packaging/PKGBUILD`，`./scripts/package-linux.sh`）包含 desktop entry
  与图标，已在 Arch / Wayland 实机验证；v0.4.2 起由 GitHub Actions 在 `v*` tag 上构建并
  附加到同名 Release。
- **Windows**：原生 MSVC 构建，`./scripts/package-windows.sh` 产出便携 zip（exe +
  `libmpv-2.dll` + FFmpeg + 许可证 + README）与 `.sha256`；v0.4.3 起由 GitHub Actions
  在 `windows-latest` 上构建并上传。
- **许可证**：GPL-3.0-or-later（ADR 0015）；`libmpv`（GPL-2.0-or-later）与 `cpal`
  （Apache-2.0）均与 GPLv3 兼容。
- **尚未完成**：Windows 安装器；文件对话框、拖放与高 DPI 的实机逐项验收；Windows 包进入 CI。
- 逐项验证记录见 `docs/history/completed-phases_ZH.md`。

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
