# 未解决问题

本文件记录已经识别、但当前阶段不解决的问题。

规则：

- 每条包含：问题、当前状态、影响、计划处理时机、解除条件。
- 已解决的问题从本文件删除，或在标题标注「已解决」并写明结论所在文档。
- 不把可以立刻查清的技术事实写在这里；先调查，再决定是否记录。
- OQ 编号保持稳定，不复用。

## 已解决编号

OQ-1…OQ-26 均已解决或失效，结论分散在 ADR 0009 / 0010 / 0013 / 0014、
`docs/investigations/` 与 `docs/history/completed-phases_ZH.md` 中。
需要逐条追溯时查看 git 历史。

OQ-27…OQ-29（Windows 构建复现性、打包自动化、运行期失败可见性）已解决：

- OQ-27 → `scripts/mpv-import-lib.sh`（说明见 `docs/development.md`）
- OQ-28 → `scripts/package-windows.sh`（说明见 `RELEASING.md`）
- OQ-29 → `logging::install_panic_hook`，日志位置见 `docs/development.md`

---

当前没有未解决的问题。
