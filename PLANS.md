# 开发计划

## 当前阶段

**Phase 2：媒体预览（已完成）**

Phase 3 尚未开始。

除非用户明确要求，不提前实现后续阶段功能。

---

# Phase 0：项目基础

状态：`完成`

## 目标

建立最小可运行 Rust + egui 项目。

## 任务

- [x] 创建 Cargo 项目
- [x] 建立模块结构
- [x] 加入 egui / eframe
- [x] 创建最小窗口
- [x] 建立错误处理
- [x] 建立日志
- [x] 建立测试结构
- [x] 建立基本 CI 检查（`scripts/check.sh`）
- [x] 创建 `AGENTS.md`
- [x] 创建 `GOALS.md`
- [x] 创建 `PLANS.md`
- [x] 创建架构文档

托管 CI：`.github/workflows/ci.yml`。Git 仓库已初始化；初始 commit 与 Phase 2 commit 已创建，Phase 2 修订尚未提交。

## 完成条件

```text
cargo check
cargo test
cargo clippy
cargo fmt --check
```

全部通过。

验证记录（2026-10-24）：

- `./scripts/check.sh` 通过，实际执行 `cargo fmt --all --check`、`cargo check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test`、`cargo build`。
- 最小窗口已在当前 Wayland 会话中实际启动，8 秒后由 `timeout` 终止。

---

# Phase 1：媒体分析

状态：`完成`

## 目标

能够打开媒体并获得可靠的媒体信息。

## 任务

- [x] FFprobe 进程封装
- [x] FFprobe JSON 解析
- [x] MediaFile
- [x] MediaFormat
- [x] VideoStream
- [x] AudioStream
- [x] SubtitleStream
- [x] Chapter
- [x] Duration
- [x] Timestamp
- [x] 错误处理
- [x] 测试媒体

## 完成条件

能够正确读取典型：

- MP4
- MKV
- WMV
- AVI
- MPEG-TS

并显示媒体基本信息。

验证记录（2026-10-24）：

- `./scripts/check.sh` 通过，实际执行 `cargo fmt --all --check`、`cargo check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test`、`cargo build`。
- 单元测试 17 个、集成测试 7 个，全部通过。
- 集成测试使用真实 FFprobe 读取 `sample.mp4`、`sample.mkv`、`sample.wmv`、`sample.avi`、`sample.ts`，断言容器、codec、尺寸、流数量、章节、非零 start time 与错误路径。
- 窗口通过 `./target/debug/rossresscut tests/fixtures/sample.mkv` 实际启动；后台探测报告 4 个流、2 个章节。
- `.github/workflows/ci.yml` 已添加，尚未在 GitHub Actions 环境实际执行。

---

# Phase 2：媒体预览

状态：`完成`

## 目标

实现流畅的视频预览。

## 任务

- [x] 确定预览后端
- [x] 设计 PreviewBackend
- [x] 播放
- [x] 暂停
- [x] Seek
- [x] 时间显示
- [x] 错误处理
- [x] 音频播放

## 注意

预览后端必须先进行架构评估。

不要因为“需要播放视频”就直接添加未经评估的播放器 crate。

当前只有一套具体后端（外部 ffmpeg 视频帧 + cpal 音频输出），因此没有引入 `PreviewBackend` trait；
未来出现第二套后端时再按真实复用需求抽取接口。

## Phase 2 修订（2026-10-24）

根据真实文件测试补齐：

- [x] 拖动时间轴时刷新预览（快速 keyframe scrub，松开后精确 seek）
- [x] 音频主时钟（FFmpeg PCM + cpal），修复独立 ffplay 进程导致的 A/V 偏移
- [x] 硬件解码默认启用：Linux VAAPI、Windows D3D11VA、macOS VideoToolbox；设备不可用时回退软件解码
- [x] Linux XDG desktop portal 文件打开
- [x] 顶部改为菜单栏（File / View / Help）
- [x] 路径显示改为标题 label，只显示不含扩展名的文件名
- [x] 预览区点击切换播放 / 暂停
- [x] 预览区与时间轴滚轮按关键帧步进，mod + 滚轮步进 10 秒
- [x] 后台关键帧索引（FFprobe packet flags，不解码视频）
- [x] 自定义时间轴与精确播放头
- [x] 侧栏播放信息（mpv Tab 风格）显示在媒体信息上方
- [x] 流信息默认折叠，并由 egui memory 记录状态
- [x] 设置页：主题下拉框 + 跟随系统复选框，语言选择
- [x] UI 本地化：`locales/*.json` 编译期嵌入，运行时选择语言
- [x] 使用 eframe storage 持久化主题、语言与 egui memory

## 已知限制

- A/V 同步由音频输出设备消费的样本帧驱动；仍受设备缓冲延迟影响（通常数十毫秒）。
- 运行环境需要 `ffmpeg`、`ffprobe` 与音频输出设备；不再需要 `ffplay`。
- 仅支持 f32 音频输出格式；设备不支持时回退墙钟并显示 warning。
- 硬件解码默认启用。Linux 只检测 `/dev/dri/renderD128`；设备不存在时使用软件解码。
- 播放信息中的 Decoder 显示请求的后端；FFmpeg 对某些 codec 可能在内部回退到软件解码，当前没有解析实际使用的解码器。
- 关键帧索引在打开媒体后于后台生成；索引完成前滚轮回退为 1 秒步进。
- 时间轴尚未按 Segment 着色；播放头与拖动已实现，Segment 颜色随 Phase 3 落地。
- 4K HEVC 精确 seek 仍可能耗时数秒；测试机 CPU 受限，不能代表一般硬件。快速 seek 调研见下节。
- A/V 同步由音频时钟驱动，但设备输出缓冲可能让时钟领先实际听到的音频；待办见下节。
- 快速 scrub 可能显示目标时间之前的关键帧；只能用于视觉反馈，不能用于 Segment 或导出。
- 预览固定 30fps、最大 960x540，不保证帧精确；非方形 SAR 尚未补偿。
- 暂无音量、音频设备选择、音轨选择。
- UI 字符串已本地化；应用层 warning 与媒体层错误仍为英文。
- 文件门户仅在 Linux 实现；其他平台暂时禁用 Open。
- 文件对话框未做自动化选择测试；已确认 XDG portal 服务与 `FileChooser` 接口存在。
- 浅色主题通过截图与 egui Context 测试验证；中文界面通过截图验证。

验证记录（2026-10-24）：

- `./scripts/check.sh` 通过，实际执行 `cargo fmt --all --check`、`cargo check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test`、`cargo build`。
- 单元测试 46 个、集成测试 14 个，全部通过；2 个音频测试默认 ignored。
- 集成测试覆盖：首帧解码、从 0.5s seek 解码、解码失败路径、快速 scrub、应用层播放 / 暂停 / seek、关键帧索引与关键帧导航。
- `cargo test --test preview_audio -- --ignored --nocapture --test-threads=1` 在带音频设备的会话中通过（2 个测试，1.94s）。
- GUI 实际加载真实 4K HEVC 素材（3840x2160, 59.94fps）并截图：菜单栏、标题 label、只读媒体信息、默认折叠的流信息、时间轴播放头与 `ffmpeg (vaapi)` 解码器状态均正常显示。
- 实际运行中后台索引了该文件的 265 个关键帧。
- 中文界面通过修改持久化语言设置后实际启动截图验证；设置窗口中的主题下拉框与语言选择可见。
- 硬件解码命令构造由单元测试覆盖（VAAPI 参数与 render node）；真实文件的 `-loglevel verbose` 输出确认选择 `hevc` 解码器并初始化 radeonsi VAAPI。
- 滚轮方向由 `wheel_direction` 单元测试覆盖；实际滚轮事件无法在无头环境注入，方向按用户反馈修正。
- 预览区空状态 / 失败状态点击打开文件已实现；未做自动化点击验证。
- `.github/workflows/ci.yml` 尚未在 GitHub Actions 环境实际执行。

## 预览性能与音画同步待办（Phase 2 后续，未开始）

### 快速 seek 调研（LosslessCut 3.70.1）

事实：

- LosslessCut 的预览不是外部 ffmpeg CLI 逐次 seek；`package.json` 没有 mpv 依赖。
- `src/renderer/src/MediaSourcePlayer.tsx` 使用 Chromium HTML5 `<video>` + Media Source Extensions：
  - master video 负责播放、`currentTime` 与音频；
  - 主进程的 `compatPlayer.createMediaSourceStream` 持续把 FFmpeg 输出写成 fragmented MP4，再 append 到 `SourceBuffer`；
  - seek 时 abort 当前流并用新的 `seekTo` 重启 ffmpeg，debounce 500ms；
  - 预览可限制到 800/420px、30/15fps，降低解码与上传成本；
  - 通过 `sourceBuffer.timestampOffset = seekTo - frameDuration` 与 30ms 间隔的 `currentTime` 同步。
- 来源：
  - https://github.com/mifi/lossless-cut/blob/master/src/renderer/src/MediaSourcePlayer.tsx
  - https://github.com/mifi/lossless-cut/blob/master/package.json

候选优化（未决定，按成本排序）：

1. 保持外部 FFmpeg，继续降低预览成本：
   - VAAPI 解码 + `scale_vaapi`，只下载缩小后的帧；
   - 降低预览分辨率 / fps，或生成低分辨率代理文件；
   - 拖动时只做 keyframe 近似，松开后再精确 seek（已实现）。
2. 换 libmpv：
   - 可能同时改善 seek、硬件解码与 A/V 同步；
   - 需要把 mpv 渲染进 egui，涉及 GL/Vulkan 互操作、窗口嵌入与跨平台。
3. 其他持久化解码器：GStreamer / libav 绑定，暂不推荐。

### 音画同步

- 当前以 cpal 实际消费的样本帧为时钟；cpal 回调写入设备缓冲不等于已经被听到，时钟可能领先一个输出缓冲周期。
- 待验证 / 待优化：
  - 减小 cpal 输出缓冲；
  - 从播放位置中扣除设备缓冲延迟；
  - 等待首个视频帧就绪后再启动音频；
  - 用真实文件做可量化的 A/V 偏移测试。
- 如果优化后仍不可接受，再评估 libmpv。

### libmpv 与 unsafe（待评估）

- libmpv 的渲染 API 需要把 mpv 的帧渲染到 egui 可用的纹理，通常要 OpenGL/Vulkan 互操作；如果由本项目代码调用 GL/FFI，会引入 `unsafe`，违反 `AGENTS.md`。
- 纯 JSON IPC（外部 mpv 进程）不引入 `unsafe`，但难以把视频嵌入 egui；Wayland 下尤其受限。
- 结论：换 libmpv 前必须先做最小实验并记录 ADR；当前不换。

## 进入 Phase 3 前注意事项

- Segment 边界必须使用精确时间与 FFprobe 数据，不得使用快速 scrub 的关键帧近似位置。
- 时间轴播放位置必须读取应用层的音频时钟；不要用 UI 墙钟自行推算播放时间。
- 关键帧索引已存在（`KeyframeIndex`），可作为 Segment 边界与导航的候选；但 keyframe 不等于用户期望的精确切点。
- 时间轴目前是单色轨道 + 精确播放头；不同 Segment 的颜色与列表编辑留给 Phase 3。
- 硬件解码默认启用。不要为了预览计时而在后续阶段默认关闭它；预览解码路径与导出路径必须保持分离。
- 4K HEVC 精确 seek 可能耗时数秒；timeline 交互需要接受该延迟，或先引入代理文件 / GPU 零拷贝预览方案。
- 保持 `Timestamp` 为整数微秒；UI 只做坐标转换，不引入浮点作为媒体时间表示。
- 预览固定 30fps / 最大 960x540，不能作为编辑或导出精度的依据。

---

# Phase 3：Timeline

状态：`未开始`

## 目标

实现核心时间轴。

## 任务

- [ ] Timestamp
- [ ] Segment
- [ ] Timeline
- [ ] 当前播放位置
- [ ] 时间轴按 Segment 着色（不同 Segment 不同颜色）
- [ ] 精确播放头与 Segment 边界联动
- [ ] In 点
- [ ] Out 点
- [ ] Segment 创建
- [ ] Segment 删除
- [ ] Segment 修改
- [ ] Segment 排序
- [ ] Segment 启用/禁用
- [ ] 键盘操作

---

# Phase 4：无损导出

状态：`未开始`

## 目标

使用 FFmpeg 实现无损 Segment 导出。

## 任务

- [ ] FFmpeg command builder
- [ ] ExportJob
- [ ] FFmpeg process runner
- [ ] progress parser
- [ ] cancellation
- [ ] 单 Segment 导出
- [ ] 多 Segment 导出
- [ ] 输出文件验证
- [ ] 回归测试

## 核心要求

不得静默从：

```text
stream copy
```

切换成：

```text
re-encode
```

---

# Phase 5：多片段合并

状态：`未开始`

## 任务

- [ ] Segment compatibility
- [ ] concat
- [ ] stream selection
- [ ] timestamp 处理
- [ ] metadata 处理
- [ ] 不兼容情况处理
- [ ] 回归测试

---

# Phase 6：缩略图

状态：`未开始`

## 任务

- [ ] 缩略图生成
- [ ] 异步处理
- [ ] 缓存
- [ ] egui texture 管理
- [ ] 时间轴显示

---

# Phase 7：波形

状态：`未开始`

## 任务

- [ ] 音频数据获取
- [ ] waveform 数据模型
- [ ] 异步生成
- [ ] 缓存
- [ ] 时间轴绘制

---

# Phase 8：高级功能

候选：

- [ ] Chapters
- [ ] Subtitle
- [ ] Stream selection
- [ ] Scene detection
- [ ] Batch processing
- [ ] Metadata
- [ ] Project persistence
- [ ] 快捷键配置
- [ ] 基于音频音量自动切分视频
- [ ] 导出视频时包含章节信息
- [ ] 智能裁切(裁切点为非关键帧时, 从该帧到关键帧进行基于此段平均码率的重新编码)

每个功能在实现前都需要独立设计。

---

# Phase 9：发布

- [ ] Linux
- [ ] Windows
- [ ] macOS
- [ ] FFmpeg 分发方案(Windows Only, macOS和Linux基本都有合适的ffmpeg安装方式)
- [ ] License
- [ ] Release build
- [ ] 自动发布
- [ ] 安装包

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
