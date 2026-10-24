# 0002 FFprobe 集成与媒体时间模型

- 状态：已接受
- 日期：2026-10-24
- 阶段：Phase 1

## 背景

Phase 1 需要从典型容器读取可靠的媒体信息，并把结果转换成与 UI、FFprobe JSON 解耦的领域模型。媒体时间必须使用整数表示，不能以 `f32` / `f64` 秒作为唯一表示。

## 决策

### 1. 直接执行 FFprobe，不链接 libav*

使用 `std::process::Command` 直接启动 `ffprobe`，每个参数单独传递，不经过 shell：

```text
ffprobe -v error -print_format json -show_format -show_streams -show_chapters <path>
```

否决 FFI / `ffmpeg-next` / 链接 libav*：项目禁止 `unsafe` 与 FFI，且媒体处理边界已经确定为外部进程。

### 2. JSON 只在解析边界内存在

`src/media/ffprobe/raw.rs` 定义私有 serde 结构，`parse.rs` 将其转换为领域模型。`serde_json::Value` 不离开该边界。

新增依赖：

- `serde`（derive）：反序列化 FFprobe JSON。
- `serde_json`：JSON 解析与错误上下文。

否决手写 JSON 解析器：FFprobe 输出字段多且存在可选字段，手写解析更容易出错。否决让 `serde_json::Value` 在应用层传播：会丢失类型约束。

FFprobe 的 `duration`、`size`、`bit_rate`、`sample_rate` 等字段实际可能以字符串或数字出现，`raw::Scalar` 在边界内统一处理。

### 3. 时间模型使用整数微秒与有理时间基

- `Timestamp`：有符号 `i64` 微秒，允许负值（容器 start time 可以为负）。
- `Duration`：无符号 `u64` 微秒，不允许负值。
- `TimeBase`：`u32/u32` 有理数，`TimeBase::MICROSECONDS = 1/1_000_000` 是内部规范基。
- 转换公式：
  - `seconds = ticks × numerator / denominator`
  - `micros = ticks × numerator × 1_000_000 / denominator`
- 转换使用 `i128` 中间值，四舍五入到最近值，正好一半时远离零；溢出返回 `None`。
- FFprobe 的十进制秒字符串由整数运算解析，最多保留 6 位小数，第 7 位四舍五入，不经过浮点。
- 章节使用 FFprobe 的整数 `start` / `end` 与 `time_base` 转换，不使用十进制 `start_time` 字符串。
- UI 显示格式为 `HH:MM:SS.mmm`。

否决 `f64` 秒模型：无法稳定表达时间戳边界、PTS/time base 转换和精确 seek。

### 4. 后台探测，UI 不阻塞

`Application::open_media` 启动命名线程并通过 `mpsc` 回传结果；UI 每帧调用非阻塞的 `Application::update` 轮询结果。探测中 UI 通过 `request_repaint_after` 定期重绘。

否决在 egui update 中同步执行 FFprobe，也否决引入异步运行时：Phase 1 只有一个短任务，标准库线程与通道足够。

打开新文件时，旧任务被替换；旧结果因接收端关闭而被丢弃，并记录 `debug` 日志。

### 5. 流分类与未知流

`MediaStream` 分为 `Video`、`Audio`、`Subtitle`、`Other`。未知 `codec_type` 不丢弃，保留基础信息为 `Other`。

`Disposition` 只建模当前 UI 需要的五个标志；其余 FFprobe 标志忽略，等有实际用途再添加。

### 6. 测试媒体由 FFmpeg lavfi 生成

`tests/fixtures/` 中的素材全部由 `generate.sh` 生成，覆盖 MP4、MKV、WMV、AVI、MPEG-TS，并包含多音轨、字幕、章节与非零 start time。集成测试调用真实 FFprobe，断言领域模型字段，不依赖 FFprobe 的日志措辞。

## 后果

- 运行与测试依赖系统中的 `ffprobe`；CI 显式安装 FFmpeg。
- 时长精度以 FFprobe 输出的 6 位小数为基础，内部表现为微秒。
- 领域模型目前只派生 `Serialize`；项目持久化阶段再添加带校验的 `Deserialize`（特别是 `TimeBase` 的零值约束）。
- Phase 1 只读取 `r_frame_rate` / `avg_frame_rate`，不会用固定帧率推导 VFR 时间。
