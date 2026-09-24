# 设计：速记左栏去掉内层文件树卡片壳

- 日期：2026-09-24
- 状态：对话已确认（方案 1）
- 分支：`feature/kb-notes-structure`
- 关联：速记 explorer 树（`NoteFolderTree` / `NoteList`）

## 1. 目标与范围

取消速记左侧**内层**文件夹树的嵌套卡片 UI（用户截图红框：`文件夹` 标题 + 树列表外的描边/底色/圆角），使树直接铺在外层「速记」玻璃卡上，避免「卡套卡」。

### 1.1 本段交付

- `NoteFolderTree.vue` 的 `.nl-tree`：去掉 `border`、`border-radius`、`--bg-card-soft` 背景（改为透明或删除对应声明）
- 保留「文件夹」文案 + FolderPlus 标题行
- 保留树行 hover / 选中 / drop-target 等交互高亮

### 1.2 本段不做

- 不去掉外层 `NoteList` 的 `.card.note-list`
- 不删「文件夹」标题文案
- 不系统改内边距/字号（非必须不调）
- 不改拖放、选中、导入等行为逻辑

## 2. 现状

| 层 | 选择器 | 视觉 |
|----|--------|------|
| 外 | `.card.note-list` | frost 玻璃卡 + 「速记」头 |
| 内 | `.nl-tree` | `border` + `radius-md` + `--bg-card-soft` ← **去掉这层壳** |

## 3. 实现要点

文件：`src/components/NoteFolderTree.vue` `<style scoped>` 中 `.nl-tree`：

- 删除或置空：`border`、`border-radius`、`background`（透明）
- 保留：`flex: 1`、`min-height: 0`、`display: flex`、`flex-direction: column`、`overflow: hidden`

无需改模板、无需改 `NoteList.vue`。

## 4. 验收

- [ ] 左栏只有一层卡片描边（外层速记卡）
- [ ] 内层树无第二层底色/圆角框
- [ ] 「文件夹」+ FolderPlus 仍在
- [ ] 选中行、hover、拖放高亮仍清晰

## 5. 架构选择

就地改 `.nl-tree` 样式（方案 1），不引入 modifier class。
