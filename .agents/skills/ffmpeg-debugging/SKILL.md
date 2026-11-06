---
name: ffmpeg-debugging
description: 用于排查 FFmpeg、FFprobe、时间戳、stream copy、容器兼容性、导出失败和音视频同步问题。
---

# FFmpeg 问题排查

## 使用场景

遇到以下问题时使用本 Skill：

- FFmpeg 导出失败
- stream copy 失败
- 时间戳异常
- 输出时长错误
- A/V sync 问题
- seek 不准确
- VFR 问题
- concat 失败
- codec/container 兼容问题

---

# 排查流程

## 1. 先复现

不要先修改参数。

确认：

- 输入文件
- FFmpeg 版本
- 完整命令
- 实际错误输出

---

## 2. 查看 FFprobe

至少检查：

- format
- duration
- start_time
- stream index
- codec
- time_base
- frame rate
- stream duration
- stream start time

---

## 3. 判断问题层级

确定问题属于：

```text
输入媒体
FFprobe
应用模型
FFmpeg 参数
FFmpeg 本身
输出容器
UI
```

---

## 4. 建立最小复现

尽可能缩小：

```text
输入
 ↓
最小 FFmpeg 命令
 ↓
错误
```

---

## 5. 修复

理解失败原因之后再修改代码。

不要随机增加：

```text
-copyts
-start_at_zero
-avoid_negative_ts
-reset_timestamps
-fflags
```

等参数。

每一个参数都必须有明确理由。

---

# 回归测试

如果发现具体媒体导致 bug：

```text
Bug
 ↓
Fixture
 ↓
Regression Test
 ↓
Fix
```

不要只验证一次命令成功。

---

# 重要原则

**FFmpeg 能处理某种媒体，不代表本项目已经正确支持这种媒体。**

必须通过项目自己的测试验证。
