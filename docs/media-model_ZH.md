# 媒体数据模型

## 一、目的

本文件定义项目中媒体相关核心数据结构。

媒体数据模型必须独立于 UI。

---

# 二、MediaFile

```text
MediaFile
├── path
├── format
├── duration
├── start_time
├── streams
└── chapters
```

代表一个实际媒体文件。

---

# 三、MediaFormat

表示容器层信息。

可能包括：

- format name
- format long name
- duration
- size
- bitrate
- start time

---

# 四、MediaStream

所有媒体流共享基础信息。

例如：

```text
index
codec
codec_name
codec_long_name
language
title
disposition
bitrate
time_base
start_time
duration
```

具体字段根据 FFprobe 实际输出确定。

不要提前创建 FFprobe 不提供的信息。

---

# 五、VideoStream

视频流额外包含：

- width
- height
- pixel format
- frame rate
- average frame rate
- codec profile
- color information

---

# 六、AudioStream

音频流额外包含：

- sample rate
- channels
- channel layout
- sample format
- bitrate

---

# 七、SubtitleStream

字幕流包含：

- codec
- language
- title
- disposition

字幕的具体处理能力根据实际功能逐步扩展。

---

# 八、Timestamp

`Timestamp` 是媒体时间轴的核心类型。

禁止在领域层使用：

```text
f32
f64
```

作为媒体时间的唯一表达。

Timestamp 必须能够：

- 比较
- 加减
- 转换
- 格式化
- 转换到指定 time base

---

# 九、Duration

Duration 表示时间长度。

必须与 Timestamp 明确区分。

---

# 十、TimeBase

FFmpeg 使用：

```text
time_base
```

表达媒体时间戳单位。

例如：

```text
timestamp × time_base = seconds
```

项目必须明确记录：

- 内部时间单位
- FFmpeg 时间单位
- UI 显示单位
- 转换规则

---

# 十一、Segment

```text
Segment
├── source
├── start
├── end
└── enabled
```

要求：

```text
start < end
```

Segment 不应该依赖 UI。

---

# 十二、Timeline

```text
Timeline
├── source
└── segments[]
```

Timeline 表达用户的编辑结果。

---

# 十三、媒体时间与 UI 时间

UI 可以显示：

```text
HH:MM:SS.mmm
```

但 UI 显示格式不是领域层的数据格式。

推荐：

```text
Domain Timestamp
        ↓
Formatter
        ↓
UI String
```

---

# 十四、VFR

可变帧率媒体不能简单假设：

```text
frame_number × constant_frame_duration
```

得到准确时间。

对于 VFR：

- 时间戳必须来自媒体实际数据。
- seek 行为必须考虑 FFmpeg 的实际语义。
- 帧导航不能简单依赖固定 FPS。

---

# 十五、PTS / DTS

项目需要区分：

- PTS
- DTS
- presentation time
- decoding time

不要将所有时间戳统一命名成 `timestamp` 后丢失语义。

---

# 十六、原则

媒体模型必须：

- 可测试
- 可序列化
- 与 egui 无关
- 与具体 widget 无关
- 尽可能与 FFprobe JSON 解耦

FFprobe 是输入来源。

`MediaFile` 才是应用层使用的正式数据模型。

---

# 十七、Phase 1 实现记录

## 1. 内部时间单位

| 类型 | 表示 | 约束 |
| --- | --- | --- |
| `Timestamp` | `i64` 微秒 | 可为负，用于容器 start time |
| `Duration` | `u64` 微秒 | 非负 |
| `TimeBase` | `u32/u32` | 分子、分母均不能为 0 |
| `FrameRate` | `u32/u32` | 分子、分母均不能为 0 |

`TimeBase::MICROSECONDS = 1/1_000_000` 是内部规范基。UI 显示单位是 `HH:MM:SS.mmm`，由领域层格式化，UI 不自行计算时间。

转换规则：

```text
seconds = ticks × numerator / denominator
micros  = ticks × numerator × 1_000_000 / denominator
```

- 使用 `i128` 中间值，避免溢出。
- 四舍五入到最近值，正好一半时远离零。
- 溢出返回 `None`，不 panic。
- FFprobe 的十进制秒字符串按整数解析：最多 6 位小数，第 7 位四舍五入，不经过 `f64`。
- 章节使用整数 `start` / `end` + `time_base`，不使用十进制字符串。
- VFR：Phase 1 只保存 `r_frame_rate` / `avg_frame_rate`，不据此推导帧时间或 seek 位置。

## 2. 实际模型字段

`MediaFile`：

```text
path: PathBuf
format: MediaFormat
duration: Option<Duration>
start_time: Option<Timestamp>
streams: Vec<MediaStream>
chapters: Vec<Chapter>
```

`MediaFormat`：

```text
name: String
long_name: Option<String>
duration: Option<Duration>
size: Option<u64>
bit_rate: Option<u64>
start_time: Option<Timestamp>
```

`StreamInfo`（所有流共享）：

```text
index: usize
codec_name: Option<String>
codec_long_name: Option<String>
profile: Option<String>
language: Option<String>
title: Option<String>
disposition: Disposition
bit_rate: Option<u64>
time_base: Option<TimeBase>
start_time: Option<Timestamp>
duration: Option<Duration>
```

流类型：

- `MediaStream::Video(VideoStream)`：额外包含宽高、pixel format、`r_frame_rate`、`avg_frame_rate`、color range/space/transfer/primaries。
- `MediaStream::Audio(AudioStream)`：额外包含 sample rate、channels、channel layout、sample format。
- `MediaStream::Subtitle(SubtitleStream)`：只包含共享字段。
- `MediaStream::Other(StreamInfo)`：FFprobe 报告的其他流类型，保留基础信息，不丢弃。

`Disposition` 当前只建模 `default`、`forced`、`hearing_impaired`、`visual_impaired`、`attached_pic`。

`Chapter`：

```text
id: i64
start: Timestamp
end: Timestamp
title: Option<String>
```

缺失字段或 FFprobe 的 `N/A` 映射为 `None`；格式错误的时间、数值、time base 或章节顺序会返回带路径与字段名的 `ProbeError::InvalidData`。

## 3. 序列化

领域模型派生 `Serialize`。项目文件（`.rrc`，ADR 0012）使用独立的持久化结构，
反序列化时校验 `TimeBase` 零值不变式、Segment 顺序与 `SegmentId` / `next_id` 一致性。

---

# 十八、播放位置（Phase 2）

播放位置属于应用状态，不属于 `MediaFile` 或 `Timeline`。

- 主路径使用 libmpv：位置来自 `time-pos`，它跟随音频输出；无音轨时同一时钟仍然有效。
- 外部 FFmpeg 回退路径由 `cpal` 实际消费的样本帧推进：

```text
position = start_position + played_frames / sample_rate
```

- 只有 mpv 不可用、无音频输出或回退路径时才使用墙钟。
- 视频帧只显示 `timestamp <= position` 的最新帧，因此音频是主时钟，视频可以丢帧但不能累积漂移。
- 快速 scrub 使用关键帧 seek，位置是近似值，不能用于 Segment 边界、编辑结果或导出。
- 释放时间轴或执行播放 seek 时必须使用精确 seek。
- 预览帧时间戳是预览时钟，不是源文件的 PTS；导出必须使用 FFmpeg 与源时间戳。

播放位置仍使用 `Timestamp`（整数微秒），不引入浮点作为领域时间表示。

---

# 十九、关键帧索引（Phase 2 修订）

`KeyframeIndex` 表示一个视频流的 keyframe 时间点，位于领域层，不依赖 FFprobe JSON 或 UI。

```text
KeyframeIndex
└── times: Vec<Timestamp>
```

规则：

- 时间戳升序、去重。
- 时间戳归一到媒体时间轴：FFprobe packet PTS 减去容器 `start_time`，负数截断为 0。
  这与预览输入 `-ss` 的相对定位语义一致。
- `previous(position)` 返回严格小于 `position` 的最近 keyframe。
- `next(position)` 返回严格大于 `position` 的最近 keyframe。
- 索引由 FFprobe 的 `packet=pts_time,flags` 生成，只读取 packet header，不解码视频。
- 索引在后台线程生成，不阻塞 UI 帧。
- Keyframe 是 seek 与后续无损裁剪的重要边界；快速 scrub 的近似位置不能替代它。

---

# 二十、Segment 与 Timeline（Phase 3）

## 1. Segment

```text
Segment
├── id: SegmentId
├── start: Timestamp
├── end: Timestamp
└── enabled: bool
```

- 区间语义为半开区间 `[start, end)`，表示单一 source 上的源时间。
- 不变式：`start < end`；`Segment::new` 不满足时返回 `None`。
- `duration()` 使用 `Timestamp::duration_since`，不经过浮点。
- 启停状态属于 Segment；修改边界时保留 ID 与启停状态。
- `SegmentId` 是项目内部的稳定身份，由 `Timeline` 分配，不依赖 Vec index。

## 2. Timeline

```text
Timeline
├── source: Option<PathBuf>
├── segments: Vec<Segment>
└── next_id: u64
```

- `segments` 是**有输出顺序的 EDL**；Vec 顺序就是用户可观察、可修改的输出顺序。
- 概念模型中的 `source` 由 Timeline 持有，而不是在每个 Segment 中重复。当前一次只打开一个媒体文件。
- 打开媒体时 `Timeline::reset(source)`，关闭媒体时 `Timeline::clear()`；两者都会重置 ID 计数器。
- `push` 追加到输出顺序末尾并分配新的 `SegmentId`；不会按起点自动排序。
- `sort_by_start` 是显式用户操作，不是不变量；排序后 SegmentId 不变。
- `move_to` / `move_up` / `move_down` 改变输出顺序，不改变源时间区间。
- `update` / `remove` / `set_enabled` 按 SegmentId 操作。
- 允许 Segment 重叠；不自动拒绝、裁剪或合并。重叠意味着同一源时间区间可以在输出中出现多次。
- ID 在同一个 Timeline 内不会复用；删除后新建的 Segment 会获得更大的 ID。

## 3. 源时间与输出时间

- `Segment.start` / `end` 是 0-based 媒体时间轴上的源时间，不是容器绝对 PTS。
- 输出时间是导出/合并时由 enabled Segment 按 Timeline 顺序累加得到的派生值，不存储在 Segment 中。
- 重叠 Segment 会增加输出总时长；disabled Segment 不参与输出。

## 4. 应用状态

以下不属于领域模型：

- 待创建的入点 / 出点（`Application` 的临时标记）。
- 当前选中的 `Option<SegmentId>`。
- Segment 命令的错误文本。

选中状态按 SegmentId 跟踪，因此排序、重排、修改边界都不会改变“选中的是哪个 Segment”。
