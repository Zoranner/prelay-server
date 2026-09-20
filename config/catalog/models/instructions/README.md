# 模型基础指令模板

按模型 id 存放语言模型的基础指令，用于填入 `language.toml` 中未配置 `base_instructions` 的条目。官方模型模板保留来源原文；项目定制模板用于补充与本地工作流匹配的行为约束。

- 命名：`<模型 id>.md`，与 `../language.toml` 的 `id` 一一对应；只按文件名精确查找，不扫描目录。`_default.md` 是保留的默认模板，供没有专属文件的模型使用。
- 来源：官方 Codex 模型模板取自 openai/codex 仓库 `codex-rs/models-manager/models.json` 的 `model_messages.instructions_template` 与同版本 `prompt.md`（默认模板），版本 `rust-v0.153.4`；DeepSeek 模板以同等完整的 Codex 运行规范为基础，叠加基于近期本地 DeepSeek 会话偏好整理的项目定制内容。
- 保真：官方模板与来源逐字节一致，不添加文件头、注释或排版调整；所有模板行尾固定 LF，且不做行尾转换。
- 同步：升级 Codex 后按同一方式重新提取，并核对长度与内容。
- 生效：服务端加载目录时按 id 读取本目录，用它填充 `base_instructions` 缺省或为空的条目；顺序为 `language.toml` 显式值 > `<id>.md` > `_default.md`。
- 体量：默认模板会套用到没有专属模板的模型；所有模板会随 `/api/catalog/models/language` 下发并由客户端写入本机模型档案。增删或替换模板后核对模型数量与字符数。

`language.toml` 的模型条目默认不配置 `base_instructions`；需要覆盖时再在条目内显式填写。

现有文件包括 `_default.md`、GPT 系列专用模板和 `deepseek-flash.md`、`deepseek-v4-pro.md` 两份 DeepSeek 专用模板；DeepSeek 两份模板当前内容一致，均包含完整 Codex 基础规范和本地用户偏好。没有专用文件的模型继续使用 `_default.md`。
