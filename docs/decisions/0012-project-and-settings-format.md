# ADR-0012：项目文件与设置文件格式

## 状态

Accepted

## 背景

Phase 7 需要把编辑结果保存下来并重新打开：源媒体路径、有序的 Segment、
以及 `Timeline` 的 ID 计数器。同时，`config.toml` 混在一起存放应用设置与
导出设置，已经影响可读性。

## 已知约束

- `Timeline` 是**有序 EDL**，顺序是用户可观察的输出顺序，不能靠
  `source time` 推导（见 ADR-0008）。
- `SegmentId` 是稳定身份，选择和编辑都依赖它；加载项目时必须原样恢复，
  否则撤销、选择和后续持久化都会错位。
- `Segment` 的不变量（`start < end`）只能由 `Segment::new` 建立。
- 项目文件是外部输入：解析失败、版本不符、ID 重复都必须报错而不是崩溃。
- 不新增依赖：`toml = "0.8"` 已经在依赖里。

## 候选方案

### 格式

A. JSON（`serde_json` 已有）
B. TOML（`toml` 已有），扩展名 `.rrc`

### 内容

A. 保存媒体元数据快照（流、时长、关键帧）
B. 只保存源路径与编辑结果，打开时重新 probe

## 决策

选择：

> TOML 格式，扩展名 `.rrc`；只保存源路径与编辑结果。

文件布局：

```toml
version = 1

[source]
path = "/media/clip.mp4"

[timeline]
next_id = 92

[[timeline.segments]]
id = 17
start = 10400000
end = 20500000
enabled = true
chapter_title = "Opening"
```

`chapter_title` 是可选的导出章节标题。旧项目没有该字段时按“未设置”处理，
因此格式版本保持 `version = 1`。

设置文件同时改为两节：`[general]`（应用设置）与 `[export]`（导出设置）。

## 决策理由

1. TOML 能直接表示 `Vec<Segment>`（`[[timeline.segments]]`）和嵌套表，
   `Timestamp` 作为整数微秒自然落位，不需要自定义序列化。
2. 与 `config.toml` 使用同一格式，人工可读、可 diff、可手改。
3. 只存编辑结果，避免媒体元数据与真实文件不一致；打开项目时重新 probe
   是唯一可信来源。
4. `version` 字段让不兼容变更可以被明确拒绝，而不是猜测。
5. `next_id` 必须保存：否则重新打开后新 Segment 可能复用旧 ID，
   破坏 `SegmentId` 的稳定性。

## 后果

### 正面

- 项目文件小、可读，能在版本控制或脚本里处理。
- 加载路径与正常打开共用 probe 流程，波形、关键帧、预览行为一致。
- 设置文件分节后，导出参数与应用参数可以独立演进。

### 负面

- 项目不记录媒体指纹；源文件被替换成同名不同内容时无法检测。
- 旧版扁平的 `config.toml` 需要兼容读取（已实现，写入时迁移为新格式）。

### 后续影响

- 新增设置项必须同时更新 `GeneralSettings` / `ExportSettings` 的默认值。
- 项目格式变更必须提升 `version` 并处理旧版本读取或明确拒绝；
  纯追加的可选字段（如 `chapter_title`）可以作为兼容变更保留版本号。

## 未解决问题

- 没有媒体指纹校验（大小 / mtime / 哈希）。
- 批量处理与多源项目尚未设计；当前 `Timeline` 仍绑定单一源媒体。
