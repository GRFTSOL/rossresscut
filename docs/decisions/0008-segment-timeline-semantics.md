# 0008 Segment / Timeline 核心语义：有序 EDL 与稳定身份

- 状态：已接受
- 日期：2026-10-24
- 阶段：Phase 3
- 补充 / 取代：`0007-segment-editing-basics.md` 中关于顺序、重叠与身份的部分

## 背景

`0007` 已经确定 Segment 是半开源时间区间、入点 / 出点属于应用层，但把“是否允许重叠”留成了孤立问题。实际实现因此处于混合状态：

- `Timeline::insert` 按起点插入；
- `replace` 又不保证顺序；
- 允许重叠；
- 选中的是 `Option<usize>`；
- 没有任何字段定义输出顺序；
- 导出尚未实现。

Phase 4 的单 Segment 导出不依赖顺序，但 Phase 5 的多 Segment 合并必须知道“哪个 Segment 先输出”。如果继续把重叠当成独立布尔值，会在导出阶段被迫重新定义模型。

## 已知事实

- `Segment` 当前为 `{start, end, enabled}`，半开区间，整数微秒。
- `Timeline` 当前为 `{source, segments}`，`insert` 按起点排序。
- 选中状态是 `Application` 中的 `Option<usize>`；排序会清空选中。
- 现有测试覆盖创建、修改、排序、启停、删除，但不覆盖顺序、重叠或导出。
- `AGENTS.md` 的 `ExportJob.segments` 是一个列表，没有单独的输出顺序字段。
- `GOALS.md` 要求单 Segment / 多 Segment 导出、Segment 排序、启停。
- `PLANS.md`：Phase 4 单段导出；Phase 5 多段合并。
- LosslessCut 的参考实现中：
  - Segment 有稳定 `segId`；
  - 列表可拖拽重排，列表顺序就是输出顺序；
  - “按起点排序”是工具操作，不是不变量；
  - 重叠是可能出现的状态，并提供检测 / 合并工具，而不是静默处理。

## 决策

选择：

> `Timeline.segments` 是有稳定输出顺序的 EDL。

具体规则：

1. **顺序即输出顺序**
   - `Vec<Segment>` 的顺序是用户可观察、可修改的输出顺序。
   - 新 Segment 追加到末尾，不按起点自动插入。

2. **Segment 表示源时间**
   - `Segment.start` / `end` 始终表示单一 source 上的源时间区间。
   - 使用 0-based 媒体时间轴（整数微秒），不是容器绝对 PTS。
   - 输出时间不存储在 Segment 中，而是导出时按 enabled Segment 顺序累加得到的派生值。

3. **enabled**
   - enabled 的 Segment 按 `segments` 顺序参与导出。
   - disabled Segment 保留在列表中，但不参与输出。

4. **重叠**
   - Segment 之间允许重叠。
   - 不自动拒绝、裁剪或合并。
   - 重叠意味着同一段源时间可以在输出序列中出现多次。

5. **排序**
   - 按起点排序是显式的用户操作，不是 Domain 不变量。
   - 排序使用稳定排序；SegmentId 与选中状态不变。

6. **稳定身份**
   - `Segment` 必须具有 `SegmentId`。
   - `SegmentId` 由 `Timeline` 分配，在同一个 Timeline 内不复用。
   - 选择、编辑、删除、重排都按 `SegmentId`，不依赖 Vec index。
   - `SegmentId` 是项目内部的轻量身份，不使用 UUID / 数据库。
   - ID 计数器在 `reset` / `clear` 时重置；项目持久化需要同时保存 ID 与计数器。

7. **Domain 支持重排**
   - Domain 提供 `move_to`、`move_up`、`move_down`。
   - 当前 UI 可以没有拖拽重排，但 Domain 不能假设列表永远按源时间排序。

8. **导出**
   - Phase 5 多 Segment 导出默认按照 Timeline 顺序解释 enabled Segment。
   - 单 Segment 导出（Phase 4）只取该 Segment 的源时间区间。
   - 本决策不规定未来 smart cut、re-encode 或重叠优化策略。

## 为什么重叠不是独立决策

重叠策略是“`segments` 是否具有输出顺序”的因变量，而不是一个可以单独选择的布尔值：

- 如果 `segments` 是有序 EDL，重叠表示同一源区间在输出序列中出现多次，是合法输入；
- 如果 `segments` 是源时间集合，重叠是集合冲突，必须拒绝或合并；
- 如果采用两层模型，重叠属于源区间集合，输出顺序属于另一层。

先决定“顺序是否即输出顺序”，重叠策略就自然确定。

## 拒绝的方案

### 源时间区间集合

始终按起点排序，重叠必须拒绝或合并。

拒绝原因：

- “Segment 排序”无法表达用户自定义输出顺序。
- 与 LosslessCut 的 EDL 行为不一致。
- 会迫使创建/修改时静默裁剪或拒绝用户边界。

### 两层模型

`Timeline` 保存源区间集合，另建 `ExportSequence` 保存输出顺序。

拒绝原因：

- 当前只有单一源媒体；
- 会提前引入两套同步模型；
- 有序 `Vec<Segment>` 已足够表达输出顺序。

## 后果

### 正面

- Phase 5 导出顺序明确：enabled Segment 按 Timeline 顺序。
- 重叠成为合法语义，不需要在创建时静默修改用户输入。
- 稳定 ID 让选择、删除、排序、未来拖拽与撤销都不再依赖索引。
- 颜色、选中状态、编辑目标都可以随 Segment 身份保持稳定。

### 负面

- `Timeline::insert` 的自动排序行为必须移除，改为追加。
- UI 列表顺序不再等于源时间顺序；时间轴仍按源时间绘制，二者需要分别理解。
- 重叠 Segment 在时间轴上会互相覆盖；列表顺序承担输出语义。
- Phase 5 必须定义/测试重叠 Segment 的输出结果（同一源区间重复出现）。
- 项目持久化需要保存 `SegmentId` 与 `next_id`。

### 后续影响

- Phase 4：`ExportJob` 使用单个 Segment，不受顺序影响。
- Phase 5：导出遍历 enabled Segment；重叠段默认重复输出。
- UI 可以后续增加拖拽重排，不需要改 Domain 语义。
- 需要时再增加“重叠警告 / 合并”作为显式命令，不能静默处理。

## 未解决问题

- 是否显示重叠警告，以及是否提供显式“合并重叠 Segment”命令。
- 拖拽重排 UI 的交互细节。
- 项目持久化的 ID 格式与 `next_id` 保存方式。
- source identity（路径 / size / mtime / hash）是否进入项目文件。
- smart cut / 非关键帧切点 / 重编码策略，均为后续独立决策。
