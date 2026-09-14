# 模型基础指令模板

按模型 id 存放语言模型的官方基础指令原文，用于填入 `language.toml` 中未配置 `base_instructions` 的条目。

- 命名：`<模型 id>.md`，与 `../language.toml` 的 `id` 一一对应；只按文件名精确查找，不扫描目录。`_default.md` 是保留的默认模板，供没有专属文件的模型使用。
- 来源：openai/codex 仓库 `codex-rs/models-manager/models.json` 的 `model_messages.instructions_template` 与同版本 `prompt.md`（默认模板），版本 `rust-v0.153.4`。
- 保真：内容与来源逐字节一致，不添加文件头、注释或排版调整；行尾固定 LF，且不做行尾转换。
- 同步：升级 Codex 后按同一方式重新提取，并核对长度与内容。
- 生效：服务端加载目录时按 id 读取本目录，用它填充 `base_instructions` 缺省或为空的条目；顺序为 `language.toml` 显式值 > `<id>.md` > `_default.md`。
- 体量：默认模板会套用到没有专属模板的第三方模型，当前 17 个语言模型的基础指令合计约 336 KiB（字符数，3 × 17730 + 21261 + 13 × 20751），随 `/api/catalog/models/language` 下发并由客户端写入本机模型档案；增删或替换模板后核对这一数值。

`language.toml` 的模型条目默认不配置 `base_instructions`；需要覆盖时再在条目内显式填写。

现有文件（字符数）：`gpt-5.6-sol.md`、`gpt-5.6-terra.md`、`gpt-5.6-luna.md` 各 17730（三条相同）；`gpt-6-astra.md` 21261；`_default.md` 20751。其余模型使用默认模板。
