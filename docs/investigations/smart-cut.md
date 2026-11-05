# Smart cut 调查（Phase 7C 前置）

状态：`调查完成，待决策后实现`

本文件只记录实际阅读到的外部行为、本项目现状与真实实验结果。
实现方案与最终取舍见 ADR（决策后补充链接）。

## 问题

普通 stream copy 的 `-ss` 起点会回退到此前关键帧。
若用户把 Segment 起点放在两个关键帧之间，输出会比请求更长，且多出的内容属于 Segment 之外。

smart cut 的目标是：

```text
请求：[5.5s, 9.0s)      关键帧：6.0s

普通 copy：  4.0s(关键帧) ──────────────── 9.0s     ← 起点回退
smart cut：  5.5s(重编码头) │ 6.0s(关键帧) ─────── 9.0s
```

即：只重编码「请求起点 → 下一个关键帧」这一小段，其余仍然 stream copy。

## 参考实现：LosslessCut 的实际行为

阅读对象：LosslessCut 主分支（GPL-2.0）。以下只描述行为，不复制代码。

### 1. 关键帧检测

- 用 `ffprobe -read_intervals <from>%<to> -show_packets -select_streams <v>
  -show_entries packet=pts_time,flags -of json` 读取起点前后 ±10 秒的包。
- 只保留 `flags` 以 `K` 开头的包。
- 起点时间与关键帧时间误差小于 `1e-6` 秒 → 视为已经落在关键帧上，不需要 smart cut。
- 否则取「起点之后的下一个关键帧」作为无损段起点。
- 找不到时把窗口扩大到 ±60 秒；仍然找不到则报错
  （`Cannot find any keyframe after the desired start cut point`）。

### 2. 判定与分段

```text
desiredCutFrom = 用户请求的起点
desiredCutTo = 用户请求的终点
losslessCutFrom = 起点之后的下一个关键帧
lastKeyframeBeforeEnd = 终点之前（或正好在终点）的关键帧

若起点已是关键帧且终点也是关键帧：
    整段普通 copy
否则若 losslessCutFrom >= 终点：
    整段重编码（片段完全位于两个关键帧之间）
否则：
    编码段 [desiredCutFrom, losslessCutFrom - 一帧)      # 起点精确
    无损段 [losslessCutFrom, lastKeyframeBeforeEnd - 两帧)
    编码段 [lastKeyframeBeforeEnd, desiredCutTo)          # 终点精确
```

编码段结束点取 `max(desiredCutFrom + 一帧, losslessCutFrom - 一帧)`，
用于避免拼接处出现重复帧，同时避免产生零长度片段。

末端的两帧余量用于避开 `-t` 按 DTS 截断的问题：如果无损段直接截在
关键帧上，FFmpeg 会保留关键帧并从下一个 GOP 多取一两个包，中间的 B 帧
却被丢掉，形成一段画面停留。让无损段提前两帧结束，再由编码段从关键帧
重新编码到终点，末端就没有空洞。

### 3. 编码段的 FFmpeg 参数（行为层面）

```text
-ss <desiredCutFrom>          # 输入 seek，前置
-i <source>
-ss 0                         # 输出侧重置；去掉会让拼接结果开头出现空洞
-t <编码段时长>
-map ...                      # 一个视频流 + 所有非视频流
-c:<视频输出> <源 codec 对应的编码器>
-b:<视频输出> <源视频码率 × 1.2>
-bf <has_b_frames>            # 仅当源报告了 B 帧
-video_track_timescale <源视频 time base>
```

- 视频编码器由源 `codec_name` 映射；AV1 映射到 `libsvtav1`，其余同名。
- 源视频 `bit_rate` 缺失时，用「文件大小 × 8 / 时长」估算，再乘 1.2。
- 只重编码**一个**视频流；音频、字幕、附加图片全部 copy。
- 无损段使用关键帧起点、`keyframeCut` 模式，并且**不**加 `avoid_negative_ts`。
- 拼接后不再整体加 `avoid_negative_ts`（见实验 3）。

### 4. 产品层面的限制（LosslessCut 自己承认）

- UI 标注 `Smart cut is experimental and will not work on all files`。
- FPS 未知时直接拒绝（需要一帧时长）。
- 只能处理恰好一个视频流。
- 只支持主文件，不支持外部附加文件。
- 码率检测失败时回退到按文件大小估算。

## 本项目现状

已有的、可以直接复用的部分：

- `domain::media::KeyframeIndex`
  - 已由 `ffprobe` 的 `-show_packets` 全片索引，后台线程加载，时间戳已归一到媒体时间轴。
  - 提供 `previous` / `next`；`next` 是**严格大于**，需要补一个「已落在关键帧上」的判断。
- `media::export::command::build_args`
  - 单段导出的参数构造；`-ss` 已前置，`-t` 在输入之后，`-c copy`。
- `media::export::concat`
  - 每段切片到临时文件 + concat demuxer；临时目录随 `Drop` 清理。
- `Application::start_export`
  - 单段/多段分流；`ExportOptions` 已承载导出设置。
- 设置项 `smart_cut` 已存在并持久化，但没有任何代码读取。

缺失的事实：

- `VideoStream` 没有建模 `has_b_frames`；需要从 FFprobe JSON 增加字段。
- 没有「源 codec → 编码器」映射。
- 没有 smart cut 的编码段构造与两段拼接。

## 实验

环境：Arch Linux，FFmpeg 系统包。素材：320x240、30fps、10 秒、H.264 + AAC，
`-g 60 -keyint_min 60 -sc_threshold 0`，关键帧在 0/2/4/6/8 秒。

### 实验 1：编码头 + 无损尾是否可行

```text
编码段：[-ss 5.5, -ss 0, -t 0.466667]  → libx264，b:v = 源码率 × 1.2
无损段：[-ss 6.0, -t 3.0]              → -c copy，无 avoid_negative_ts
concat：-f concat -safe 0 -map 0 -c copy（无 avoid_negative_ts）
```

结果：

| 项目 | 结果 |
| --- | --- |
| 编码段 | 14 帧，0.466667s |
| 无损段 | 92 帧，3.066667s（请求 3.0s，多 2 帧） |
| 拼接输出 | 106 帧，3.539974s |
| 视频接缝 | 无间隙；帧间隔稳定在 0.0333s |
| 末尾 | 比请求多约 0.0667s，来自无损段本身的末端溢出 |

### 实验 2：起始内容是否真的精确

抽取拼接输出的第一帧，与源文件在 5.5s / 6.0s 的帧比较 PSNR：

| 对比 | PSNR |
| --- | --- |
| 输出首帧 vs 源 5.5s 帧 | 36.58 dB |
| 输出首帧 vs 源 6.0s 帧 | 11.07 dB |

结论：输出确实从请求的 5.5s 开始，而不是回退到 6.0s 的关键帧。
36.58 dB 的差异来自编码段的重编码损失。

### 实验 3：concat 时加 `avoid_negative_ts` 的后果

编码段首帧有负 DTS（B 帧）。concat 时加 `-avoid_negative_ts make_zero`：

```text
video start_time = 0.066667
首帧 PTS = 0.066667        ← 开头出现约 2 帧空洞
```

不加时：

```text
video start_time = 0.000000
首帧 PTS = 0.000000，DTS = -0.066667
```

结论：smart cut 的输出不能整体套用 `-avoid_negative_ts make_zero`。
当前普通多段导出使用该参数是安全的（每段输入已被 copy 归零），
但 smart cut 路径必须区分。

### 实验 4：编码段去掉 `-ss 0`

```text
首帧 PTS = 1.019987        ← 保留了输入 seek 的内部偏移
输出时长 4.559961s         ← 比请求多出约 1 秒
```

结论：编码段的 `-ss 0` 不可省略。

### 实验 5：`-video_track_timescale` 对 Matroska 输出

Matroska 输出接受该参数，不报错。是否真的生效不影响可行性判断。

## 事实 / 推论 / 决策

### 事实

1. 编码段可以精确从请求起点开始，前提是输入 seek + 解码丢弃（`-ss` 前置 + `-ss 0`）。
2. 编码段与无损段可以直接 concat，接缝无间隙。
3. 末端同样可以精确：用编码段覆盖最后一个关键帧到请求终点的区间。
4. 无损段不能直接截在关键帧上；`-t` 按 DTS 截断会保留关键帧却丢掉中间的
   B 帧，留下 2–4 帧的画面停留。提前两帧结束再由编码段接管可以避免。
5. 拼接 smart cut 片段时不能加 `avoid_negative_ts make_zero`。
6. Matroska 的 cue 点可能比关键帧稀疏。`-ss <关键帧>` 在 copy 模式下可能
   落到更早的 cue，把关键帧之前的内容一起复制。实测 Inception HEVC MKV：
   `-ss 60.56` 落到 `53.22`，`-ss 61.06` 才落到 `60.56`。
   规划阶段用 `ffmpeg -ss <target> -copyts ... -frames:v 1` + ffprobe
   探测实际落点；找到 `落点 >= 请求起点` 的目标后，编码头延长到落点，
   copy 段使用该目标。找不到时整段编码。
7. 当前项目已经拥有全片关键帧索引，不需要像 LosslessCut 那样按窗口现查。
8. `has_b_frames` 未建模，但可以从 FFprobe JSON 补上。

### 推论

1. 多段导出时，每段可能产生「编码头 + 无损中段 + 编码尾」三个子片段；
   可以把所有子片段按 Timeline 顺序放进同一个 concat list。
2. 编码段和无损段的流布局、codec、时间基必须一致，否则 concat 失败。
3. 源 codec 没有对应软件编码器时，smart cut 无法完成。
4. 该功能本质上是「局部重编码」，输出不再是严格意义上的
   「所有帧都原样复制」，必须在产品文案与导出结果中明确。

### 需要项目负责人决定

见下方「待决策」。

## LosslessCut 对照（2026-11-04）

对照 `mifi/lossless-cut` master 的 `smartcut.ts`、`useFfmpegOperations.ts`、
`util/streams.ts`（只读参考，未复制代码）：

- 它的 smart cut 只处理**起点**：`needsSmartCut()` 找起点之后的关键帧，
  `cutEncodeSmartPart()` 编码 `[请求起点, 该关键帧)`，剩余区间继续 copy。
  片段终点仍由 `-t` copy 截断，没有尾部编码。
- Matroska + B 帧的提前 seek（issue #1216）用固定补偿：
  `getKeyframeCutSeekCompensation()` 在 `-ss` 上加 `3/23 + 0.001` 秒，
  再用 `-itsoffset` 把输出零点移回原切点。
- 导出参数与我们的编码头一致：`-ss` 在 `-i` 之前、编码段再加 `-ss 0`、
  `-t` 在输入之后、concat demuxer 合并、码率 ×1.2。

与本项目的差异：

- OQ-25：本项目额外编码**尾部最后一个 GOP**；LosslessCut 不做，其段尾仍
  依赖 `-t` copy 截断。两者属于同一思路（局部重编码 + copy 中间段 +
  concat），本项目把范围扩展到终点。
- OQ-26：本项目用实际 seek 落点探测替代固定补偿。固定补偿只覆盖约 0.13s
  的 Matroska 偏移；实测稀疏 cue 可提前 7s 以上，因此必须按落点规划。
  `-itsoffset` 的方案也会平移音频时间戳，探测 + 延长编码头不改变音画时间轴。

## 待决策

### D1：smart cut 不可用时怎么办

触发条件（任一）：

- 找不到起点之后的关键帧；
- FPS 未知（无法计算一帧时长）；
- 选中的视频流多于一个；
- 源视频 codec 没有可用的软件编码器；
- 片段完全位于两个关键帧之间（整段需要重编码）。

候选：

- **A. 严格失败**：中止导出并给出具体原因。与 LosslessCut 一致，
  符合「不得静默降级」。
- **B. 回退到普通 copy**：起点回退到此前关键帧，并在结果中明确标注
  「本段未使用智能裁切，起点已回退」。导出仍然成功。

### D2：编码质量目标

- **A. 跟随 LosslessCut**：源视频码率 × 1.2，自动，不提供 UI。
- **B. 提供自定义码率/质量**：增加导出设置项。

推荐 A。smart cut 的重编码范围通常只有几百毫秒到几秒，
自动跟随源码率已经足够，额外的质量控制属于后续优化。

## 2026-11-05 VFR 与图形字幕验证

### VFR

合成 VFR 素材：120/60/40fps 三段，带 B 帧，关键帧每 30 帧。
智能裁切区间 `[4.1,7.2)` 与 `[8.1,10.2)`。

实现：

- 编码段加 `-fps_mode passthrough`，避免 FFmpeg 把 VFR 重采样成猜测的 CFR。
- 用 `r_frame_rate` 与 `avg_frame_rate` 相差 >0.5% 判定 VFR。
- 规划线程在切点 ±0.5s 读取包 PTS，取最大相邻间隔作为该边界的局部帧时长；
  编码头/尾分别使用起点与终点的局部时长。
- 无损段提前 2 个局部帧结束，编码尾从该结束点开始（不是从关键帧开始），
  这样视频没有空洞，音频也不跨越整段关键帧间隔。

实测（B 帧 VFR）：最大视频间隔 25ms（40fps 段正常帧间隔），
最大音频接缝 72ms；无 B 帧 VFR 的音频接缝约 27ms。
AAC 包 21.3ms，接缝空隙受包边界限制，已在 ADR-0013 记录为已知限制。
回归：`tests/smart_cut_export.rs::smart_cut_preserves_vfr_timestamps_and_audio_cadence`。

### 图形字幕

FFmpeg 没有 PGS/DVD/DVB 编码器，用 GStreamer `dvbsubenc` 生成
`tests/fixtures/dvb_subtitles.mkv`（80x60、10fps、3s、DVB subtitle）。

智能裁切区间 `[0.2,2.8)` 会拆成编码头 + 无损中段 + 编码尾；
输出 DVB 字幕 40 个包，时间范围 `0..2.532s`，视频 `0..2.466s`，
没有超出输出或跨接缝的明显空洞。图形字幕保持 stream copy，
`-ss` 与 concat 会把它和视频一起重定基。

回归：`tests/smart_cut_export.rs::smart_cut_keeps_graphical_subtitles_in_range`。
PGS / DVD 未直接实测，但走同一条非文本字幕 copy 路径。
