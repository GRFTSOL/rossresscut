# 媒体处理规则

本文件是媒体处理、FFprobe、媒体时间、Segment、无损编辑与导出的约束来源。数据模型见 `docs/media-model.md`，分层见 `docs/architecture.md`。

## 1. FFmpeg 是媒体处理后端

不要重新实现：

- 编解码器
- 容器解析
- 音视频编码
- mux
- demux
- 时间戳处理的完整替代实现

这些工作交给 FFmpeg。

两种调用方式并存：

- 导出、裁切、合并与探测：外部 FFmpeg / FFprobe 进程。
- 预览解码与音频输出：进程内 `mpv-engine`（libmpv）。

两者都必须使用 FFmpeg / libmpv 的能力，不允许自写编解码器或容器解析。预览的进程内边界与 zero-copy 规则见 `docs/decisions/0010-preview-via-mpv-engine.md`。

Rust 负责：

- 命令构造
- 参数验证
- 进程启动
- 进程管理
- 进度解析
- 取消
- 错误处理
- 媒体模型
- 应用层逻辑

---

## 2. FFmpeg 调用方式

导出、裁切、合并与探测必须直接执行外部程序：

```text
Command
 ↓
FFmpeg executable
```

避免：

```text
Shell
 ↓
FFmpeg
```

不要通过 shell 拼接用户输入。

文件路径、参数等必须作为独立进程参数传递。

预览解码与音频使用进程内 `mpv-engine`（libmpv），同样不得经由 shell，也不得在 egui 帧内同步调用。

---

## 3. 预览与音频

- 预览主路径使用 `mpv-engine`（libmpv）解码、渲染与音频输出；外部 FFmpeg 预览保留为回退路径。
- 播放位置使用 libmpv 的 `time-pos`，它跟随音频输出；无音轨时同一时钟仍然有效。
- 音频由 libmpv 的 `ao` 输出；`cpal` 只在外部 FFmpeg 回退路径使用，不得与 mpv 同时输出音频。
- 拖动时间轴使用 `hr-seek=no`（落在前一个关键帧）提供视觉反馈；松开时使用指针最终位置并切换 `hr-seek=yes` 做精确 seek。
- `PlaybackRestart` 事件缺失时，精确 seek 以 `position()` 落点兜底，容差为 100 ms。
- 快速 scrub 的位置是近似值，不得用于 Segment 边界、编辑结果或导出。
- 硬件解码由 libmpv 自动选择（`hwdec=auto-safe`）；设备不可用时自动回退软件。
- 预览导出尺寸按预览窗格大小与源宽高比等比适配，按物理像素下发，且不得超过源分辨率（不放大）。
- 解码失败或 mpv 启动失败必须可见；播放信息显示 `mpv (<hwdec-current>)`，
  例如 `mpv (vaapi)`、`mpv (vaapi-copy)`、`mpv (no)`；外部回退路径显示 `ffmpeg (...)`。
- 预览区点击切换播放 / 暂停；滚轮按关键帧步进（上滚上一个，下滚下一个），按住 mod 键时步进 10 秒。
- 预览不保证帧精确；导出与编辑不得依赖预览时间。
- 精确 seek 在目标纹理到达前不得绘制旧位置纹理；UI 显示加载状态，并有超时
  回退避免画面卡死。快速 scrub 不受此门控影响。

---

## FFprobe

FFprobe 输出必须尽快转换成明确的 Rust 数据结构。

推荐：

```text
FFprobe JSON
    ↓
解析器
    ↓
MediaFile
    ├── Format
    ├── VideoStream
    ├── AudioStream
    ├── SubtitleStream
    └── Chapter
```

不要让 `serde_json::Value` 在整个项目中到处传播。

FFprobe 的 JSON 只应该存在于媒体解析边界。

---

## 时间系统

媒体时间是本项目的核心数据。

### 禁止

不要在核心逻辑中使用：

```rust
f32
f64
```

作为唯一的媒体时间表示。

---

### 原因

视频编辑涉及：

- 帧边界
- 时间戳
- PTS
- DTS
- time base
- VFR
- 音视频同步
- 精确 seek
- 容器时间戳

浮点秒数容易产生精度和边界问题。

---

### 要求

项目必须定义明确的：

- `Timestamp`
- `Duration`
- `TimeBase`

并明确它们之间的转换规则。

具体设计记录在：

```text
docs/media-model.md
```

---

## Segment

片段是用户编辑行为的核心数据。

概念模型：

```text
Segment
├── id (SegmentId)
├── start
├── end
└── enabled
```

片段必须能够脱离 UI 单独存在。

不能依赖：

```text
某个 egui widget
某个拖动状态
某个临时 UI 数值
```

来表达最终编辑结果。

当前实现将 `source` 绑定在 `Timeline` 上，而不是在每个 `Segment` 中重复保存。
一次只编辑一个源文件；打开新文件会清空 Timeline。

`Timeline.segments` 是**有输出顺序的 EDL**：列表顺序就是导出顺序，按起点排序是显式操作，不是不变量。
Segment 允许重叠，不自动拒绝、裁剪或合并。
选择、编辑、删除与重排必须使用稳定 `SegmentId`，不得依赖 Vec index。
详见 `docs/decisions/0008-segment-timeline-semantics.md`。

---

## 无损编辑

项目的核心功能是**无损媒体编辑**。

默认情况下优先使用：

```text
stream copy
```

而不是重新编码。

如果某项操作需要重新编码：

- 必须明确知道原因。
- 必须明确告诉用户。
- 不允许静默切换到重新编码。
- 不能把重新编码操作标记成无损操作。
- 必须显式传递源的 `pix_fmt` 与 color range / space / transfer / primaries，
  保留 10-bit/HDR 元数据；不得用预览的 8-bit 帧作为重编码输入。

### 降级原因必须具体

降级原因必须是可展示给用户的具体文本，例如"切点不在关键帧上，smart cut 需要重新编码"，
不能只用一个布尔标志表示"会发生重编码"。

判断是否降级的那一层负责给出原因文本；UI 只负责展示，不自行拼接或改写原因。

多段 concat 不改写包时间戳。导出先在后台探测每段的首包 PTS 与末包结束时间，
再按“本段结束 - 下一段首 PTS”写入 concat list 的 `duration`，这样下一段首帧
紧跟上一段末帧，不会插入 1 帧间隙。音频包时间戳保持原样。
不得把多段输出描述为帧精确：非关键帧起点仍会包含此前内容，非关键帧结束点
在含 B 帧的素材上可能留下短暂画面停留。开启智能裁切后，起点与终点由编码段
补齐，段尾不再有该停顿；但被重编码的头/尾必须明确告知用户。用户可见说明见
README 与导出面板。

---

## 导出

导出任务至少应该包含：

```text
ExportJob
├── source
├── segments
├── stream_selection
├── output
├── mode
├── progress
├── cancellation
└── result
```

导出必须支持：

- 进度
- 错误
- 取消
- 输出文件检查

导出未成功结束之前，不得将任务标记为成功。

### 流选择

- 流选择是可选的裁剪：未指定时保持源文件的全部流（`-map 0`）。
- 一旦指定，就只映射列出的**绝对流索引**，不依赖相对顺序。
- 流选择优先于字幕替换：用户没有勾选字幕轨时，即使已经裁剪出字幕，也不输出字幕轨。
- 字幕内容始终是**裁剪后**的，流选择只决定该轨是否出现。
- 各段拼接要求流布局一致；不一致时不得静默丢弃流来“凑”成功。

---
