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

领域模型当前派生 `Serialize`。`Deserialize` 留到项目持久化阶段，并需要校验 `TimeBase` 的零值不变式。
