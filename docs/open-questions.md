# 未解决问题

本文件记录已经识别、但当前阶段不解决的问题。

规则：

- 每条包含：问题、当前状态、影响、计划处理时机、解除条件。
- 已解决的问题从本文件删除，或在标题标注「已解决」并写明结论所在文档。
- 不把可以立刻查清的技术事实写在这里；先调查，再决定是否记录。
- OQ 编号保持稳定，不复用。已删除的编号：OQ-1（设备选择委托给 mpv，见
  `docs/architecture.md`）、OQ-2、3、4、5、6、7、8（方案 D 失效或已被 ADR 0010
  取代）、OQ-9（10 分钟长时复测通过，见
  `docs/investigations/playback-pipeline.md`）、OQ-10（窗口缩放验证与 GPU 回归
  完成）、OQ-11（音轨/设备切换语义与输出延迟已核查）、OQ-12（scrub 与
  frame-step 已完成）、OQ-13（HDR/10-bit 预览与导出验证完成）、OQ-14（拼接
  偏移已按真实时间戳修正，见
  `docs/investigations/multi-segment-concat.md`）、OQ-21（精确 seek 旧帧门控已修复，均见
  `docs/investigations/mpv-engine-spike.md`）、OQ-15、20（已解决）、
  OQ-19（已实现）、OQ-25（尾部 smart cut 已实现，见
  `docs/decisions/0013-smart-cut.md`；纯 copy 仍保留段尾 B 帧空洞）、
  OQ-26（规划阶段探测 seek 落点，稀疏 cue 容器可用 smart cut）。历史见
  `docs/decisions/0009-preview-decode-zero-copy.md`、
  `docs/decisions/0010-preview-via-mpv-engine.md`、`docs/decisions/0013-smart-cut.md`
  与 git 历史。

---

## 多段导出（来源：Phase 5 / `docs/investigations/multi-segment-concat.md`）

### OQ-16 逐项 metadata 与源章节编辑

**问题**：除输出标题外，没有逐项 metadata 编辑；容器自带章节只读。

**当前状态**：Stream selection、章节标题编辑与项目持久化已实现；导出章节自动生成；
metadata 只写输出 `title`。

**影响**：不能编辑语言、默认轨、注释等 metadata；不能编辑或导入源容器章节。

**计划时机**：metadata 需求出现时。

**解除条件**：按独立设计实现需要的 metadata 字段，以及源章节的导入/覆盖语义。

### OQ-17 4K / 60fps / VFR 验证

**问题**：多段合并目前只在 1080p30 H.264/AAC 上验证。

**当前状态**：字幕与章节已有导出测试；4K / 60fps / VFR 素材未测。

**影响**：高帧率或 VFR 素材的拼接间隙、时间戳行为可能不同。

**计划时机**：Phase 5 回归扩展或发布前回归。

**解除条件**：至少覆盖 4K60、VFR，以及含字幕/章节容器的端到端拼接。

### OQ-18 多段导出的 UI 交互自动化

**问题**：导出按钮、进度、取消只做了启动冒烟，没有自动化 UI 交互测试。

**当前状态**：`scripts/ui_a11y.py` 已提供 AT-SPI 读取与点击基础设施，导出流程尚未接入。

**影响**：UI 回归只能靠人工。

**计划时机**：需要稳定的 UI 回归时。

**解除条件**：能自动触发导出并断言进度、完成与取消状态。

## 智能裁切（来源：Phase 7C / `docs/decisions/0013-smart-cut.md`）

### OQ-22 智能裁切的编码后端

**问题**：当前只使用软件编码器（`libx264` / `libx265` / `libsvtav1` 等）。
硬件编码器（NVENC / VAAPI / VideoToolbox）未接入。

**当前状态**：未实现。

**影响**：长片段重编码时 CPU 占用高；没有硬件编码器的机器无法加速。

**计划时机**：出现真实性能需求时；必须先决定是否允许依赖 GPU 编码能力。

**解除条件**：有 ADR 记录硬件编码的使用边界与降级语义。

### OQ-23 智能裁切的 VFR 与音画接缝

**问题**：VFR 素材用平均帧率计算一帧时长；编码段与无损段的音频接缝未测量不连续。

**当前状态**：未验证。

**影响**：VFR 下可能多丢或少丢一帧；接缝处可能有一帧级的音频不连续。

**计划时机**：有 VFR 素材或用户报告音画问题时。

**解除条件**：有基于源 PTS 的帧时长计算，或实测证明接缝不可感知。

### OQ-24 智能裁切与图形字幕

**问题**：智能裁切把片段拆成两个子片段，图形字幕（PGS/DVD/DVB）仍按原样 stream copy。

**当前状态**：未验证。

**影响**：图形字幕可能出现在错误的子片段边界，或 concat 后错位。

**计划时机**：有图形字幕素材的真实需求时。

**解除条件**：有测试覆盖图形字幕的智能裁切导出。
