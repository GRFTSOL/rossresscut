# 多 Segment 无损合并调查（Phase 5 前置）

状态：`调查完成，未实现`

本文件只记录实际运行的实验与结论。实现在 Phase 5 进行。

## 问题

Timeline 是有序 EDL：

```text
Timeline
 ├─ Segment A [10s, 20s)
 ├─ Segment B [35s, 42s)
 └─ Segment C [70s, 80s)
        ↓
    A → B → C
```

需要在不重新编码的前提下，把多个 source-time 区间按 Timeline 顺序写成一个文件。

## 参考实现

LosslessCut 的做法：

1. 每个 Segment 单独导出为临时文件（`-ss` before `-i`、`-t`、`-map`、`-c copy`、`-avoid_negative_ts make_zero`）。
2. 用 concat demuxer 合并：

```text
ffmpeg -f concat -safe 0 -protocol_whitelist file,pipe,fd -i -
       -map ... -c copy -ignore_unknown -f <format> -y <output>
```

3. concat list 通过 stdin 传入。
4. metadata 从第一个片段单独输入；chapters 走 ffmetadata 输入。

不使用 concat filter，因为 concat filter 需要解码并重新编码。

## 实验环境

- FFmpeg 9.0.2
- 素材：生成的 20 秒 1080p30 H.264 + AAC，`-g 60 -keyint_min 60 -sc_threshold 0`，关键帧每 2 秒
- 请求区间：
  - p1：`[3.3, 7.3)`，起点不是关键帧
  - p2：`[10.7, 13.7)`，起点不是关键帧

## 实验 1：MP4 临时片段 + concat demuxer

```text
ffmpeg -ss 3.3 -i src.mp4 -t 4 -map 0 -c copy -avoid_negative_ts make_zero p1.mp4
ffmpeg -ss 10.7 -i src.mp4 -t 3 -map 0 -c copy -avoid_negative_ts make_zero p2.mp4
ffmpeg -f concat -safe 0 -i list.txt -map 0 -c copy -avoid_negative_ts make_zero out.mp4
```

结果：

| 项 | 视频 | 音频 | 容器时长 |
| --- | --- | --- | --- |
| p1（请求 4s） | 5.300s / 159 帧 | 5.333s / 250 包 | 5.333s |
| p2（请求 3s） | 3.700s / 111 帧 | 3.733s / 175 包 | 3.733s |
| out | 9.033s | 9.067s | 9.067s |

关键观察：

- 请求 `[3.3, 7.3)` 实际输出 **5.333 秒**，起点回退到关键帧 2.0。
- 请求 `[10.7, 13.7)` 实际输出 **3.733 秒**，起点回退到关键帧 10.0。
- 输出时长是各段实际时长之和，不是 `sum(end - start)`。
- 拼接处视频 PTS：`5.282682 → 5.349349`，间隔 66.7ms（正常帧间隔 33.3ms），即约 33ms 的间隙。
- 音频在拼接处连续：`5.312000 → 5.333333 → 5.354667`。
- 视频首帧 PTS 是 `0.016016`，不是 0。

## 实验 2：concat demuxer 的 `inpoint/outpoint`

### 2a：关键帧对齐

```text
file 'src.mp4'
inpoint 2
outpoint 7.3
file 'src.mp4'
inpoint 10
outpoint 13.7
```

结果：输出 9.033s，视频 9.000s / 270 帧，音频 9.033s。拼接处视频 `5.282682 → 5.316016`，间隔正常。

但 FFmpeg 报 `Non-monotonic DTS`，并自动改写时间戳。

### 2b：非关键帧对齐

```text
inpoint 3.3
outpoint 7.3
inpoint 10.7
outpoint 13.7
```

结果：输出 8.333s，拼接处出现大量 `Non-monotonic DTS`；第二段视频 PTS 以 `1/15360` 步进（`5.282747, 5.282813, 5.282878...`），时间基完全错乱。

**结论：`inpoint/outpoint` 不可用于本项目。**

## 实验 3：TS 临时片段

```text
ffmpeg -ss 3.3 -i src.mp4 -t 4 -map 0 -c copy -f mpegts p1.ts
ffmpeg -ss 10.7 -i src.mp4 -t 3 -map 0 -c copy -f mpegts p2.ts
ffmpeg -f concat -safe 0 -i list.txt -map 0 -c copy -avoid_negative_ts make_zero out_ts.mp4
```

结果：

- p1：视频 5.300s，音频 5.312s
- p2：视频 3.700s，音频 3.605s（音频比视频短 95ms）
- out：9.049s，视频 9.016s，音频 9.049s
- 拼接处视频 `5.282667 → 5.332000`，间隔 49.3ms

TS 没有消除间隙，并引入了音频/视频长度不一致。

## 实验 4：MKV 临时片段

```text
ffmpeg -i src.mp4 -c copy src.mkv
ffmpeg -ss 3.3 -i src.mkv -t 4 -map 0 -c copy -avoid_negative_ts make_zero m1.mkv
ffmpeg -ss 10.7 -i src.mkv -t 3 -map 0 -c copy -avoid_negative_ts make_zero m2.mkv
ffmpeg -f concat -safe 0 -i list.txt -map 0 -c copy -avoid_negative_ts make_zero out_mkv.mkv
```

结果：

- 片段 start_time：视频 0.000，音频 0.026
- out：9.076s
- 拼接处视频 `5.267000 → 5.338000`，间隔 71ms

MKV 不比 MP4 好。

## 实验 5：`-shortest`

在切片时加 `-shortest`：

```text
ffmpeg -ss 3.3 -i src.mp4 -t 4 -map 0 -c copy -avoid_negative_ts make_zero -shortest p1_short.mp4
```

结果：

- p1：视频 5.300s，音频 5.312s
- p2：视频 3.700s，音频 3.712s
- out：9.032s，视频 9.016s，音频 9.028s
- 拼接处视频 `5.282682 → 5.332031`，间隔 49.3ms

相比基线，额外间隙从约 33ms 降到约 16ms。

## 内容验证：起点回退

提取源在 2.0s、3.3s 的帧，以及 p1 的第一帧：

```text
ffmpeg -i src.mp4 -ss 2.0 -frames:v 1 src_2.png
ffmpeg -i src.mp4 -ss 3.3 -frames:v 1 src_3.png
ffmpeg -i p1.mp4 -frames:v 1 p1_first.png
md5sum
```

结果：

```text
9819a768...  src_2.png
5be3bfaf...  src_3.png
9819a768...  p1_first.png
```

p1 的第一帧等于源 2.0s 的关键帧，而不是请求的 3.3s。

## 结论

### 方案选择

| 方案 | 无损 | 结论 |
| --- | --- | --- |
| 临时片段 + concat demuxer | 是 | **采用** |
| concat demuxer `inpoint/outpoint` | 是 | 非关键帧会破坏时间戳，不用 |
| concat protocol | 是 | 仅适合 MPEG-TS 等，不通用 |
| concat filter | 否（重编码） | 不用 |

### 已知语义

1. **起点回退到关键帧**：Segment 起点不是关键帧时，输出会包含从该关键帧到请求起点之间的内容。
2. **输出时长不是 `sum(end - start)`**：等于各段实际片段时长之和。
3. **拼接处有 ≤1 帧的视频间隙**：约 16–33ms，音频连续。stream copy 无法消除。
4. **视频首帧 PTS 可能非零**：MP4 下约 16ms；这是 B 帧 DTS 归零的副产物。
5. **MP4 与 MKV 行为不同**：MKV 的音频起点偏移 26ms，拼接间隙更大；输出容器应与源一致。
6. **流必须一致**：同一 source 的片段满足；不同容器/流布局不支持。
7. **DTS 非单调警告**：concat demuxer 可能改写时间戳，需要记录而不是忽略。

### Phase 5 实现建议

1. `ExportRequest` 从单段扩展为按 Timeline 顺序的 Segment 列表。
2. 每个 Segment 先写到输出同目录或系统临时目录的临时文件，扩展名与输出一致。
3. 切片使用 `-ss` before `-i`、`-t`、`-map 0`、`-c copy`、`-avoid_negative_ts make_zero`、`-shortest`。
4. 用 concat demuxer 合并，list 通过 stdin 或临时文件传入。
5. 临时目录使用 RAII guard，成功、失败、取消都删除。
6. 进度按各段实际输出时长加权；总时长需要从切片结果读取，不能只用 `sum(end-start)`。
7. 顺序严格使用 `Timeline.segments()`，不得按 `start` 重新排序。
8. 重叠 Segment 按顺序重复输出，不自动合并或裁剪。
9. 流一致性在开始前检查；不一致时明确失败，不静默重编码。
10. UI 必须显示起点回退与拼接间隙的语义，不能把结果描述成帧精确。

### 未验证

- 60fps / 4K 素材的拼接间隙是否仍为一帧。
- VFR 素材。
- 含字幕、章节、attached_pic 的容器。
- 不同容器之间的 concat（例如 MP4 片段 → MKV 输出）。
- 音频 priming 在多次 concat 后的累积偏移。
