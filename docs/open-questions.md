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
  OQ-26（规划阶段探测 seek 落点，稀疏 cue 容器可用 smart cut）、OQ-16（用户
  决定不做复杂 metadata 与源章节编辑，现有功能足够）、OQ-17（4K60、1080p60、
  VFR、字幕与章节端到端拼接验证通过，见
  `docs/investigations/multi-segment-concat.md`）、OQ-18（导出 UI 由
  `scripts/ui_export_smoke.py` 自动断言运行、完成与取消状态）、OQ-22（保持软件
  编码；preset 与码率见
  `docs/decisions/0014-smart-cut-encoding-defaults.md`）、OQ-23（VFR 帧时长改为
  按切点附近 PTS 计算，编码段使用 `-fps_mode passthrough`；见
  `docs/decisions/0013-smart-cut.md`）、OQ-24（DVB 图形字幕 smart cut 回归见
  `tests/smart_cut_export.rs::smart_cut_keeps_graphical_subtitles_in_range`）。
  历史见
  `docs/decisions/0009-preview-decode-zero-copy.md`、
  `docs/decisions/0010-preview-via-mpv-engine.md`、`docs/decisions/0013-smart-cut.md`
  与 git 历史。

---

当前没有未解决问题。
