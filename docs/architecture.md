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

# 二、UI 层

负责：

- 绘制界面
- 接收用户输入
- 显示应用状态
- 显示任务进度
- 显示错误

不负责：

- FFmpeg 命令
- FFprobe JSON
- 媒体解析
- Segment 业务逻辑
- 文件导出

---

# 三、应用层

应用层表达用户操作。

例如：

```text
OpenMedia
CloseMedia
Seek
CreateSegment
UpdateSegment
DeleteSegment
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
Timestamp
Duration
Segment
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

媒体层不得依赖 egui。

---

# 六、任务系统

长时间运行的工作：

```text
FFprobe
FFmpeg
Thumbnail
Waveform
Scene Detection
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
ExportProgress
ExportStatus
Error
```

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
0001-rust-native.md
0002-preview-backend.md
0003-timestamp-model.md
0004-export-strategy.md
```

---

# 十一、当前代码结构（Phase 1）

```text
src/
├── main.rs                    二进制入口：日志初始化、启动窗口、错误退出
├── lib.rs                     库根：导出各层模块
├── error.rs                   应用错误类型（上下文 + source 链）
├── logging.rs                 进程级日志初始化
├── application/mod.rs         Application、OpenState、后台探测任务
├── domain/
│   ├── media.rs               MediaFile / MediaFormat / 流 / Chapter
│   └── time.rs                TimeBase / Timestamp / Duration / FrameRate
├── media/
│   └── ffprobe/
│       ├── mod.rs             FFprobe 进程封装与 ProbeError
│       ├── raw.rs             私有 serde 结构（JSON 边界）
│       └── parse.rs           JSON → 领域模型
└── ui/mod.rs                  egui/eframe 窗口与媒体信息展示
```

依赖方向：

- `main` → `ui`、`logging`、`error`
- `ui` → `eframe`、`application`、`domain`（只读领域状态）
- `application` → `domain`、`media::ffprobe`、`error`
- `media` → `domain`、外部 FFprobe 进程
- `domain` → 标准库、`serde`；不得依赖 `eframe`、`egui`、FFprobe JSON
- `ffprobe::raw` 只在 `ffprobe` 模块内部可见

打开媒体流程：

```text
UI 路径输入 / 命令行参数
        ↓
Application::open_media
        ↓
后台线程 → ffprobe::probe
        ↓
raw JSON → parse → MediaFile
        ↓
Application::poll（非阻塞）
        ↓
UI 渲染
```

egui 帧内不会执行 FFprobe，也不会阻塞等待通道。

验证入口：

```bash
./scripts/check.sh
```
