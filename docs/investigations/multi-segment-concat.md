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
3. **拼接偏移按真实时间戳计算**：不再插入额外间隙；30fps 实测帧间隔 33.3ms。
   非关键帧结束点在纯 stream copy 下仍可能留下 B 帧重排空洞；启用
   smart cut 后由编码尾补齐（ADR-0013）。
4. **视频首帧 PTS 可能非零**：MP4 下约 16ms；这是 B 帧 DTS 归零的副产物。
5. **MP4 与 MKV 行为不同**：MKV 的音频起点偏移 26ms，拼接间隙更大；输出容器应与源一致。
6. **流必须一致**：同一 source 的片段满足；不同容器/流布局不支持。
7. **DTS 非单调警告**：concat demuxer 可能改写时间戳，需要记录而不是忽略。

### 2026-11-04 拼接偏移修正（已实现）

用 640x360@30、H.264 + AAC、关键帧每 2 秒的素材复测，请求两段
`[3.3,7.3)` 与 `[10.7,13.7)`，基线视频拼接处是
`5.282682 → 5.349349`，间隔 66.7ms；正常帧间隔 33.3ms，即额外约 1 帧。

失败过的方案：

- 切片加 `-shortest`：间隔降到 49.3ms；但源带附加图片/字幕流时会提前结束，
  不能作为默认行为。
- 只给 concat list 加 `duration`：视频间隔压到 24–33ms，但音频包重叠，
  出现 `Non-monotonic DTS`。
- 逐流 `setts` 重定基：B 帧会让视频首 DTS 为负，`make_zero` 又把整段平移
  回去；且不同流首 PTS 不同时，逐流重定基会破坏音画同步。

最终方案：**不改写任何包时间戳，只修正 concat list 的 `duration`**。

1. 切片保持 `-ss` before `-i`、`-t`、`-map 0`、`-c copy`、`-avoid_negative_ts make_zero`。
2. 切片全部完成后，在工作线程逐段探测视频首包 PTS 与最大 `PTS + duration`。
3. concat list 中每段的 `duration` =
   `本段视频最大结束时间 - 下一段视频首包 PTS`；最后一段用本段跨度。
4. concat 仍用 `-avoid_negative_ts make_zero`；它只做一次全局平移，
   所有段一起移动，不改变相对拼接。

实测（320x240@30、H.264 + AAC、关键帧每 1 秒，请求 `[3.3,7.3)`、
`[10.7,13.7)`）：

- 视频：`4.267 → 4.300 → 4.333`，间隔 33.3ms，无停顿。
- 音频：`4.296 → 4.300 → 4.323`，单调递增。
- 输出时长 8.038s（音频尾部比视频长约 19ms，属于 AAC 包边界差）。

真实 HDR 素材（HEVC 23.976fps + AAC，带 B 帧）实测：

- 第二段首帧正好接在第一段视频末包之后（`4.796 → 4.837`，41ms），
  拼接处没有额外插入的间隙。
- 但第一段自身的末尾有 `4.671 → 4.796`（125ms）的画面停留。这是切片
  `-t` 按 DTS 截断、丢掉引用链中间 B 帧造成的，与 concat 偏移无关，
  已记录在 ADR-0013；smart cut 的编码尾可以消除段尾空洞。

跨度末端取所有包中最大的 `PTS + duration`，不能取 mux 顺序最后一行：
B 帧重排会让最后一个 mux 包的 PTS 不是最大 PTS。
Matroska 的 stream duration 是 `N/A`，不能用 `probe().duration`。

拼接偏移不能简单地用每一段自己的跨度：`-ss` 切出的段首 PTS 不一定是 0，
相邻段可能不同。用 `结束时间 - 下一段首 PTS` 才能让下一帧紧接着上一帧。

结论：**实现“切片后探测 + concat 偏移修正”**，拼接处不再插入帧间隙。
实现见 `src/media/export/concat.rs` 的 `start_probing_timings` /
`concat_duration`，回归测试见
`tests/multi_export.rs::joins_parts_without_a_frame_gap`。

### 2026-11-05 4K / 60fps / VFR / 字幕章节验证

环境：FFmpeg 9.0.2，素材来自 `/home/chasel/videos`。

验证走项目真实代码路径：多段导出使用 `MultiExport`；smart cut 使用
`Application::open_media` + `start_export`。拼接间隙按“上一段末包结束时间”
与“下一段首包 PTS”计算，不把段尾空洞算进拼接间隙。

| 素材 | 区间 | 拼接处视频间隔 | 纯 copy 额外停顿（视频段尾 / 音频接缝） | 输出时长 |
| --- | --- | --- | --- | --- |
| 4K60 HEVC（3840x1920, 59.94） | `[10.010,15.015)` + `[30.030,35.035)` | 16.0ms | 67ms 视频 / 110ms 音频 | 10.278s |
| 1080p60 HEVC（1920x886, 59.94） | 同上 | 16.0ms | 67ms 视频 / 110ms 音频 | 10.278s |
| 4K60 AV1 / WebM（3840x1934） | `[4.033,8.333)` + `[13.300,14.967)` | 16.0ms | 无 | 5.988s |
| VFR（合成 120/60/40fps 三区段） | `[1.0,4.0)` + `[7.0,10.0)` | 8.0ms | 无 | 6.026s |
| 奥本海默 1080p HEVC 23.976 + 2 条文本字幕 + 20 章节 | `[57.391,67.818)` + `[72.531,82.958)` | 41.0ms（一帧） | 250ms 视频 / 231ms 音频 | 27.567s |

观察：

1. 所有格式的**拼接处**都只隔一个正常帧（60fps 约 16ms、23.976fps 约 41ms），
   没有额外插入的 1 帧间隙。
2. 60fps / HEVC 素材在纯 copy 下仍有段尾 B 帧空洞，表现为视频 67ms、
   音频接缝 110ms；这是 ADR-0013 已记录的段尾语义，不属于拼接间隙。
   开启 smart cut 后消失：1080p60 与 4K60 实测最大视频间隔 17ms、
   最大音频间隔 21.33ms，输出 10.113s。
3. AV1/WebM 与 VFR 素材的纯 copy 路径没有段尾空洞，拼接处同样正常。
4. VFR 素材用合成文件验证：源帧间隔为 8.33 / 16.67 / 25.00ms，
   导出后平均间隔 11.97ms、最大 25ms、无时间戳回退。
   `bilibili` 目录下没有找到真正的 VFR 文件（全部为 CFR）。
5. 含字幕与章节素材：输出生成 2 个章节、替换为 1 条裁剪后的字幕轨、
   6 个字幕包；源字幕流未残留。

发现并修复的问题：`MultiExport` 把裁剪后的 SRT 作为输入传给 FFmpeg，
但没有在 `ExportRequest` 上设置 `with_subtitle`，导致 `build_args` 仍然
`-map 0` 并复制源字幕流，SRT 输入没有被映射。修复后每个切片都映射
替换后的字幕轨；没有 cue 的片段写入一个 1ms 的空占位 cue，避免
concat 时流布局不一致。回归测试：
`tests/multi_export.rs::multipart_export_replaces_source_subtitles`。

结论：4K60、60fps、VFR、文本字幕与章节容器的端到端拼接均已覆盖，
OQ-17 解除。

### Phase 5 实现建议

1. `ExportRequest` 从单段扩展为按 Timeline 顺序的 Segment 列表。
2. 每个 Segment 先写到输出同目录或系统临时目录的临时文件，扩展名与输出一致。
3. 切片使用 `-ss` before `-i`、`-t`、`-map 0`、`-c copy`、`-avoid_negative_ts make_zero`。
4. 切片完成后逐段探测首包 PTS 与末包结束时间，再用 concat demuxer 合并；
   list 中每段的 `duration` 用“本段结束 - 下一段首 PTS”计算。
5. 临时目录使用 RAII guard，成功、失败、取消都删除。
6. 进度按各段实际输出时长加权；总时长需要从切片结果读取，不能只用 `sum(end-start)`。
7. 顺序严格使用 `Timeline.segments()`，不得按 `start` 重新排序。
8. 重叠 Segment 按顺序重复输出，不自动合并或裁剪。
9. 流一致性在开始前检查；不一致时明确失败，不静默重编码。
10. UI 必须显示起点回退的语义，不能把结果描述成帧精确。

### 未验证

- 不同容器之间的 concat（例如 MP4 片段 → MKV 输出）；本次只测了输入输出同容器。
- 音频 priming 在多次 concat 后的累积偏移。
- attached_pic 的单独复测（当前保持 stream copy）。

smart cut 的 VFR 与图形字幕已另行验证：见 `docs/investigations/smart-cut.md`
的「2026-11-05 VFR 与图形字幕验证」。
