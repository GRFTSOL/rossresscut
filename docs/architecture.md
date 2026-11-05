# 项目架构

## 一、总体架构

```text
                    ┌──────────────┐
                    │    egui UI   │
                    └──────┬───────┘
                           │
                           ▼
                    ┌──────────────┐
                    │    应用层     │
                    │ Application  │
                    └──────┬───────┘
                           │
                           ▼
                    ┌──────────────┐
                    │    领域层     │
                    │    Domain    │
                    └──────┬───────┘
                           │
                           ▼
                    ┌──────────────┐
                    │    媒体层     │
                    │    Media     │
                    └──────┬───────┘
                           │
                           ▼
                    ┌──────────────┐
                    │ FFmpeg/FFprobe│
                    └──────────────┘
```

---

## 依赖方向

允许：

```text
UI
 ↓
Application
 ↓
Domain
 ↓
Media
 ↓
FFmpeg / FFprobe（外部进程：导出 / 探测）
libmpv（进程内：预览解码与音频）
```

禁止：

- Domain → UI / egui
- Media → UI / egui
- FFmpeg 输出 → UI
- `media::preview` 的 GPU 纹理 / FFI 句柄类型 → Domain / Application / UI

核心媒体逻辑必须能在不启动图形界面的情况下测试。

---

# 二、UI 层

负责：

- 绘制界面
- 接收用户输入
- 显示应用状态
- 显示任务进度
- 显示错误
- 通过 XDG desktop portal 触发 Linux 文件选择
- 显示只读媒体标题、播放信息与设置窗口
- 选择界面语言并读取本地化字符串（`ui/i18n.rs`）

不负责：

- FFmpeg 命令
- FFprobe JSON
- 媒体解析
- Segment 业务逻辑
- 文件导出
- 自建文件选择器
- 计算播放位置
- 构造 FFprobe 关键帧查询

---

# 三、应用层

应用层表达用户操作。

例如：

```text
OpenMedia
CloseMedia
Play
Pause
Seek
ScrubPreview
StepKeyframe
Skip
SetInPoint
SetOutPoint
CreateSegment
UpdateSegment
DeleteSegment
MoveSegment
SortSegments
ToggleSegment
SelectStream
ExportSegments
CancelExport
```

UI 发送命令。

应用层处理命令。

---

# 四、领域层

领域层表达媒体编辑本身。

主要模型：

```text
MediaFile
MediaFormat
MediaStream
VideoStream
AudioStream
SubtitleStream
Chapter
KeyframeIndex
Timestamp
Duration
Segment
SegmentId
Timeline
ExportRequest
```

领域层不得依赖 egui。

---

# 五、媒体层

负责：

- FFprobe
- FFmpeg
- 进程管理
- 参数构建
- 进度解析
- 取消
- 媒体操作
- 预览视频帧解码（`mpv-engine` / libmpv，进程内，GPU-resident）
- 预览音频输出（libmpv `ao`）
- 关键帧索引（外部 ffprobe 读取 packet flags，不输入解码）
- 文本字幕提取、裁剪与重新 mux（`media/subtitle.rs`）
- 场景检测（`media/scene.rs`，外部 FFmpeg 只输出时间戳）
- 导出结果合并（`media/export/merge.rs`，只 remux）

Phase 4B 修订后的预览边界（ADR 0010）：

```text
libmpv（mpv-engine，进程内）
        ↓ decoded frame (export)
      wgpu external texture
        ↓
media::preview 导入为 wgpu::Texture
```

导出、裁切、合并与探测仍使用外部 FFmpeg / FFprobe 进程。
详见 `docs/decisions/0010-preview-via-mpv-engine.md`。

导出流程：

```text
单 Segment
  → ffmpeg -ss before -i -t -map 0 -c copy -avoid_negative_ts make_zero

多 Segment
  → 每个 Segment 切片到临时文件
  → ffprobe 读取每个临时片段的实际时长
  → 用实际时长生成章节（起点回退到关键帧，请求时长不等于实际时长）
  → ffmpeg -f concat -safe 0 -i list -map 0 -c copy
  → 临时目录在成功 / 失败 / 取消后删除
```

切片不使用 `-shortest`：源文件带有附加图片或字幕流时，它会让 FFmpeg
在最短流处提前结束，产物可能近乎为空。各段长度由 `-t` 控制。

多段顺序只来自 `Timeline.segments()`；不得按 `start` 重新排序。
拼接语义与已知间隙见 `docs/investigations/multi-segment-concat.md`。

媒体层不得依赖 egui。

`cpal` 只在外部 FFmpeg 回退路径负责音频 PCM 输出；主路径音频由 libmpv `ao` 输出。

预览解码设备选择委托给 libmpv：主路径使用 `hwdec=auto-safe`，Linux 上 mpv 提供
`--vaapi-device`（默认 `/dev/dri/renderD128`）。项目不枚举 GPU，也不在 UI 中
暴露设备选择；需要指定设备时属于后续功能，不在当前范围内。Windows D3D11
适配器选择未验证，当前不承诺。

---

# 六、任务系统

长时间运行的工作：

```text
FFprobe
FFmpeg export / probe (external process)
Preview decode (libmpv via mpv-engine, in-process)
Keyframe indexing
Audio output (libmpv ao; cpal only in the FFmpeg fallback)
Waveform
Scene Detection
Silence Detection
Batch / Merge export
```

不得在 egui update 中同步执行。

基本模型：

```text
UI
 ↓
Command
 ↓
Application
 ↓
Job
 ↓
Worker
 ↓
FFmpeg
 ↓
Event
 ↓
Application State
 ↓
UI
```

---

# 七、状态

## 领域状态

表达真正的媒体编辑结果。

例如：

```text
Timeline
Segment
StreamSelection
```

## 应用状态

表达程序运行状态。

例如：

```text
PlaybackState
PlaybackPosition
KeyframeIndex
PendingInPoint
PendingOutPoint
SelectedSegmentId
ExportProgress
ExportStatus
Error
```

播放位置属于应用状态。有音频时由音频输出设备消费的样本帧推进；无音频或音频设备不可用时才使用墙钟。

## UI 状态

表达界面交互状态。

例如：

```text
HoveredSegment
DraggingHandle
OpenPanel
```

三种状态必须保持清晰边界。

---

# 八、核心原则

## 1. Domain 不认识 UI

Domain 不允许：

```rust
use egui::*;
```

---

## 2. Media 不认识 UI

Media 不允许依赖：

```text
egui
eframe
UI widget
```

---

## 3. UI 不直接执行 FFmpeg

所有媒体操作必须经过应用层。

---

# 九、功能实现顺序

新增功能优先遵循：

```text
需求
 ↓
领域模型
 ↓
应用命令
 ↓
媒体操作
 ↓
测试
 ↓
UI
```

不要从：

```text
egui widget
```

开始设计整个功能。

---

# 十、架构决策

重大架构决策记录在：

```text
docs/decisions/
```

格式：

```text
编号-标题.md
```

例如：

```text
0001-foundation-stack.md
0002-ffprobe-integration.md
0003-preview-backend.md
0004-audio-clock-and-desktop-integration.md
0005-hardware-decoding.md
0006-ui-localization-and-settings.md
0007-segment-editing-basics.md
0008-segment-timeline-semantics.md
0009-preview-decode-zero-copy.md
0010-preview-via-mpv-engine.md
0011-waveform-in-timeline.md
0012-project-and-settings-format.md
0013-smart-cut.md
0014-smart-cut-encoding-defaults.md
0015-project-license.md
```

---

# 十一、项目、批处理与派生数据

## 项目文件（ADR 0012）

- `.rrc` 保存源路径、有序 Segment、`SegmentId` / `next_id` 与章节标题。
- 不保存媒体元数据、关键帧、波形与场景检测结果；打开项目时重新 probe。
- 源文件缺失时只读加载，允许重新绑定后才能导出。
- 自动保存按编辑后 2 秒节流；关闭媒体 / 窗口 / 应用时若有未保存修改则询问。

## 批处理与合并

- 批处理对队列中的每个文件套用当前 Timeline 的 enabled Segment。
- 每个文件在后台 probe + 导出，输出到源文件旁或导出目录；UI 不阻塞。
- 合并先导出各文件，再用 concat demuxer remux；不重新编码，不生成章节。
- 批处理与合并都不替换当前打开的媒体。

## 派生数据

- 波形、关键帧索引、场景检测与静音检测结果都是派生数据，不进入项目文件。
- 波形：外部 FFmpeg 解码为 `s16le`，折叠为 min/max peak（ADR 0011）。
- 场景检测：外部 FFmpeg `select=gt(scene,N),showinfo`，只解析时间戳。
- 静音切分：复用波形 peak，在静音区间中点分割。
- 所有派生数据都在后台线程生成，UI 只轮询结果。

---

# 十二、当前代码结构（Phase 7）

```text
src/
├── main.rs                    二进制入口：日志初始化、启动窗口、错误退出
├── lib.rs                     库根：导出各层模块
├── error.rs                   应用错误类型（上下文 + source 链）
├── logging.rs                 进程级日志初始化
├── application/
│   ├── mod.rs                 Application 状态、OpenState、查询与导出入口
│   ├── media.rs               打开媒体、FFprobe 轮询、关键帧后台索引
│   ├── transport.rs           play / pause / seek / scrub / step / skip
│   ├── editing.rs             标记、选择、剪贴板、撤销/重做、Segment 命令
│   ├── playback.rs            Playback 播放引擎：时钟、解码、mpv 后端与回退
│   ├── waveform.rs            波形后台提取、`has_audio` 与 `poll_waveform`
│   ├── export.rs              ExportStatus、ExportJob（单段 / 多段）、导出命令错误
│   ├── batch.rs               批处理队列：对多个文件逐个 probe + 导出
│   ├── chapters.rs            导出章节标题（按 `SegmentId` 保存）
│   ├── project.rs             `.rrc` 项目文件保存 / 恢复与源文件重新绑定
│   ├── scene.rs               后台场景扫描、状态与结果写入 Timeline
│   └── silence.rs             静音检测自动建片段（复用波形 peak）
├── domain/
│   ├── media.rs               MediaFile / MediaFormat / 流 / Chapter / KeyframeIndex
│   ├── segment.rs             Segment / SegmentId / Timeline（有序 EDL、稳定身份）
│   ├── time.rs                TimeBase / Timestamp / Duration / FrameRate
│   ├── waveform.rs            Peak / Waveform（音频包络与按列聚合）
│   └── silence.rs             静音区间检测与中点切分规则
├── media/
│   ├── export/
│   │   ├── mod.rs             ExportProcess、进度/结束事件、取消与回收
│   │   ├── command.rs         ExportRequest 与 FFmpeg stream-copy 参数
│   │   ├── concat.rs          多段切片、concat demuxer、临时目录生命周期
│   │   ├── progress.rs        `-progress pipe:1` 解析
│   │   ├── chapters.rs        `ffmetadata` 章节文件生成
│   │   ├── merge.rs           已导出文件的 concat 合并（只 remux）
│   │   ├── planning.rs        多段智能裁切规划与 seek 落点探测
│   │   └── smart_cut.rs       智能裁切分段：编码头/尾 + 无损中段
│   ├── ffprobe/
│   │   ├── mod.rs             FFprobe 进程封装与 ProbeError
│   │   ├── raw.rs             私有 serde 结构（JSON 边界）
│   │   ├── parse.rs           JSON → 领域模型
│   │   ├── keyframes.rs       packet flags → 关键帧索引
│   │   ├── packets.rs         包首/末时间戳边界（拼接对齐）
│   │   └── seek.rs            探测输入 seek 的实际落点
│   ├── preview/
│   │   ├── mod.rs             PreviewError、共享进程清理与参数格式化
│   │   ├── mpv.rs             mpv-engine 封装：生命周期、export、帧获取、命令与事件
│   │   ├── video.rs           回退路径：ffmpeg RGB 帧解码、硬件解码选择、背压、seek
│   │   └── audio.rs           回退路径：ffmpeg PCM 解码 + cpal 输出
│   ├── scene.rs               外部 FFmpeg 场景检测（只解析时间戳）
│   ├── subtitle.rs            文本字幕提取、裁剪与重新 mux
│   └── waveform.rs            ffmpeg → s16le，折叠为 min/max peak
└── ui/
    ├── mod.rs                 RossresscutApp 状态、窗口入口与 `eframe::App`
    ├── actions.rs             快捷键分发、片段输入框同步
    ├── timeline.rs            时间轴与波形绘制、播放控件、滚轮与坐标换算
    ├── preview.rs             预览绘制、texture 上传、渲染诊断
    ├── panels.rs              右侧播放信息、片段与章节面板、导出状态行
    ├── menu.rs                菜单栏
    ├── windows.rs             设置、关于、导出面板、批处理、对话框窗口
    ├── settings.rs            UiSettings、config.toml 读写、主题检测与 `apply_theme`
    ├── shortcuts.rs           ShortcutAction、Shortcut 解析与默认绑定
    ├── fonts.rs               字体发现与字体选择器
    ├── dialogs.rs             XDG 门户文件/目录选择与关闭确认
    ├── diagnostics.rs         测试用诊断驱动（预览尺寸、自动播放等）
    ├── icons.rs               Nerd Font 图标字形常量
    └── i18n.rs                语言枚举、强类型字符串与嵌入资源
```

locales/
├── en.json
└── zh-CN.json
```

依赖方向：

- `main` → `ui`、`logging`、`error`
- `ui` → `eframe`、`application`、`domain`（只读领域状态）；Linux 下 → `rfd`
- `ui::i18n` → 嵌入的 locale JSON、`serde`
- `application` → `domain`、`media::ffprobe`、`media::preview`、`error`
- `media` → `domain`、外部 FFmpeg / FFprobe 进程、`mpv-engine`（预览主路径）、
  `cpal`（仅在外部 FFmpeg 回退路径）
- `domain` → 标准库、`serde`；不得依赖 `eframe`、`egui`、FFprobe JSON
- `ffprobe::raw` 只在 `ffprobe` 模块内部可见

打开媒体流程：

```text
UI Open（Linux: rfd xdg-portal，后台线程） / 命令行参数
        ↓
Application::open_media
        ↓
后台线程 → ffprobe::probe
        ↓
raw JSON → parse → MediaFile
        ↓
Application::update（非阻塞）
        ↓
后台线程 → ffprobe probe_keyframes（packet flags）
        ↓
KeyframeIndex
        ↓
播放状态机 / UI 渲染
```

预览与时钟流程：

```text
UI Play / Pause / Seek / ScrubPreview / StepKeyframe / Skip
        ↓
Application
        ├── MpvPreview（libmpv：解码 + ao + seek）
        │   └── export → wgpu texture（GPU-resident，无 CPU 回读）
        └── 回退：PreviewVideo（ffmpeg → RGB24）
                   + PreviewAudio（ffmpeg → PCM → cpal）
        ↓
mpv time-pos（跟随音频输出）= 播放位置
        ↓
Application::update（非阻塞轮询事件与位置）
        ↓
UI texture / 时间轴 / 播放信息 / 警告
```

快速 scrub 流程：

```text
时间轴拖动
        ↓
暂停播放（记录拖动前是否在播放）
        ↓
Application::scrub → mpv hr-seek=no + seek
        - 落点为前一个关键帧，约 80ms 限流
        ↓
释放时间轴（使用指针最终位置）
        ↓
Application::seek → mpv hr-seek=yes + 精确 seek
        ↓
如果拖动前正在播放，则 Application::play
```

快速 scrub 位置只用于视觉反馈，不用于编辑边界或导出。

egui 帧内不会执行 FFprobe、FFmpeg、mpv 命令或音频解码，也不会阻塞等待通道。

UI 布局：

```text
顶部：菜单栏（File / View / Help）、设置按钮、当前媒体标题（不含扩展名）
中央：视频预览（点击切换播放 / 暂停，滚轮按关键帧步进）
右侧：播放信息（mpv Tab 风格）在上；Segment 列表（输出顺序，添加切点 / 删除切点 / 上移 / 下移 / 分割）在下
底部：时间轴单独一行；下方是播放控制（13 个控制按钮 + 音量按钮 + 片段起止时间输入框）；右下角为 Export 按钮
```

设置页：

```text
常规：主题、界面语言、字体与缩放、波形 / 保留被截断字幕等
导出：输出目录、章节开关、智能裁切与码率
快捷键：可分组折叠、可录制、可为空
```

主题、语言与 egui memory（流折叠状态、窗口尺寸）通过 eframe storage 持久化。

时间轴滚轮步进：

```text
上滚 → 上一个关键帧
下滚 → 下一个关键帧
mod + 滚轮 → ±10 秒
```

关键帧索引在后台生成；索引未完成时滚轮回退为 ±1 秒。

滚轮交互占用了 command / ctrl 修饰键，因此 egui 的 ctrl+滚轮界面缩放被显式关闭（`InputOptions::zoom_modifier = Modifiers::NONE`）。

Segment 编辑流程：

```text
UI: Alt+I / Alt+O / Alt+B / Delete / Buttons
        ↓
Application: set_in_point / set_out_point / add_segment
        ↓
Domain: Timeline::push(start, end) → SegmentId
        ↓
UI: Segment 列表（输出顺序）、时间轴颜色、入点 / 出点标记
        ↓
Delete / 修改边界 / 启停 / 排序 / 上移 / 下移 → Application 命令（按 SegmentId）
```

- 入点 / 出点与选中的 `SegmentId` 属于应用状态，不属于 `Timeline`。
- `Timeline.segments` 的顺序就是输出顺序；新 Segment 追加到末尾，按起点排序是显式命令。
- 允许 Segment 重叠，不自动拒绝、裁剪或合并。
- 时间轴点击 / 拖动仍使用 `Seek` / `ScrubPreview`；点击同时按 SegmentId 选中点击位置所在的 Segment。
- 详见 `docs/decisions/0008-segment-timeline-semantics.md`。

布局方向记录在 `GOALS.md`。

验证入口：

```bash
./scripts/check.sh
```
