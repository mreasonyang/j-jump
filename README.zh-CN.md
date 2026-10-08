<div align="center">

# J-Jump

**在终端里按名称、按含义、用任何语言，一步跳到想去的目录。**

[![最新版本](https://img.shields.io/github/v/release/mreasonyang/j-jump?sort=semver)](https://github.com/mreasonyang/j-jump/releases/latest)
[![许可证：MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.md)
![平台：macOS | Linux](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey)
![Shell：Bash | Zsh | Fish](https://img.shields.io/badge/shell-bash%20%7C%20zsh%20%7C%20fish-4EAA25)

[English](README.md) · **简体中文**

</div>

<p align="center"><img src=".github/assets/demo.gif" alt="J-Jump 演示：j pay 按名称跳转；ji my cv 经 Jev 找到 resumes；ji 税务 找到 taxes" width="760"></p>

你从没输入过 “resumes” 或 “taxes”。J-Jump 会记住你去过的目录：记得名字，几个字母就能跳到；记不清名字，可选的语义模型会按含义帮你找到，中英文混用也没问题。

> [!NOTE]
> J-Jump 还处于早期阶段（0.0.x 版本），难免有不完善之处；升级后偶尔需要对配置或访问记录做一次性处理，`jjump doctor` 会告诉你具体怎么做。欢迎在 [Issues](https://github.com/mreasonyang/j-jump/issues) 中反馈。

## 为什么选择 J-Jump

- **按名称，即刻到达。** `j pay` 会在你真正去过的目录中找到最佳匹配并直接跳转。排序完全在本机完成，不联网。
- **按含义，模型由你选。** 想不起目录叫什么？`ji my cv` 会请语义模型判断你要找的是哪个目录：可以用云端的 Jev、Cloudflare Clef-Flash 或 OpenAI，也可以用完全运行在你电脑上的 Tev1。
- **任何语言都能用。** `ji 税务` 能找到 `taxes`，`ji machine learning experiments` 能找到 `机器学习实验`。
- **决定权始终在你。** 语义功能默认关闭，开启后也只提建议，你不选就不会跳转。默认情况下，模型只能看到你的查询词和目录名。
- **小巧、原生。** 单个 Rust 可执行文件，支持 macOS 和 Linux 上的 Bash、Zsh、Fish。无需额外运行时、插件管理器，本地导航也不需要后台服务。

## 快速开始

**1. 安装。** 下面任一命令都会同时接入你的 Shell。

```sh
# Homebrew
brew install mreasonyang/taps/j-jump && jjump shell install

# 或使用安装脚本（macOS 15+ 或 Linux，x86-64 或 ARM64）
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh | sh
```

安装脚本会校验 SHA-256，然后把 `jjump` 安装到 `~/.local/bin`，不需要 sudo。Shell 接入会先备份启动文件，再添加一段带标记的配置；加上 `--no-shell` 可跳过。[更多选项](docs/CONFIGURATION-AND-HELP.md)

**2. 打开一个新终端**，照常使用 `cd`。J-Jump 的访问记录一开始是空的，会在你每次进入目录时自动学习。

**3. 运行 `j` 或 `ji`。** 第一次运行会弹出简短的设置向导。选择 `off` 就只用本地功能，不需要任何 Key；也可以选一个语义服务。之后随时可以用 `jjump setup` 修改。

> `j` 已经被别的命令占用了？用 `jjump shell install --cmd jump`，改为提供 `jump` 和 `jumpi`。

## 日常使用

| 命令 | 作用 |
| --- | --- |
| `j pay` | 跳到与 `pay` 最匹配的已访问目录 |
| `j api server` | 多个关键词按顺序匹配路径 |
| `j api /` | 只在当前目录之下查找 |
| `j`、`j -` | 回到主目录，或回到上一个目录 |
| `j ../dir`、`j -- 'my folder'` | 也可以直接写路径 |
| `ji` | 用编号列表浏览全部访问记录 |
| `ji pay` | 从匹配结果中选择；开启语义功能后会先询问语义服务 |
| 输入 `j pay ` 后按 <kbd>Tab</kbd> | 把选中的目录填进命令行，按 <kbd>Enter</kbd> 才跳转 |

目录名完全相同的排在最前，其次是前缀、包含和上级目录匹配；同一档内，常去和最近去过的目录优先。运行 `jjump explain pay` 可以查看原因。

## 按含义找目录

语义功能**默认关闭**。在 `jjump setup` 中选一个服务，向导会同时引导你填写 Key 或本地模型：

| 服务 | 运行位置 | 你需要准备 |
| --- | --- | --- |
| TypeSafe AI 的 [Jev](https://docs.typesafe.ai/introduction)（默认） | 云端 | 在 [TypeSafe 控制台](https://console.typesafe.ai/keys)创建的 API Key |
| Cloudflare Workers AI 上的 [Clef-Flash](https://developers.cloudflare.com/workers-ai/models/clef-flash/) | 云端 | Cloudflare Account ID 和 Workers AI API Token |
| [OpenAI Decisions](https://developers.openai.com/api/docs/guides/decisions)（`gpt-6-luna`） | 云端 | OpenAI API Key，以及 J-Jump 0.0.42+ |
| 通过 [Ollama](https://ollama.com/download) 运行的 Tev1 4B | 你的电脑 | Ollama 0.35+、约 4.5 GB 的模型，以及 J-Jump 0.0.40+ |

- `j` 只有在本地完全没有匹配时才会询问语义服务；开启语义功能后，`ji 查询词` 每次都会询问。
- 语义服务推荐的目录排在第一位并带有标注。**始终由你来选**，不会替你选中任何目录。
- 默认每次请求前都会征求你的同意。云端服务可能按请求计费。
- 默认的 `strict` 隐私模式只发送查询词和候选目录名。文件内容、Git 远程地址、环境变量、Shell 历史和 Key 永远不会发送。用 `jjump preview "my cv"` 可以查看完整请求。
- 敏感目录可以加入 `no_send`（本地可搜索，但不发送）或 `exclude`（完全不记录）。

不用向导的配置方法、各隐私级别和全部设置项，见[配置、帮助与恢复](docs/CONFIGURATION-AND-HELP.md)。

## 常见问题

- **提示 `j: command not found`：** 打开一个新终端。如果仍然不行，运行 `jjump shell install`。
- **`j foo` 找不到目录：** J-Jump 只认识安装之后去过的目录，先 `cd` 进去一次。
- **其他问题：** 运行 `jjump doctor`，它离线运行并告诉你下一步怎么做。

更多解决办法和退出码见[配置指南的故障排查](docs/CONFIGURATION.md#troubleshooting)（英文）。

## 更新与卸载

```sh
# 更新
brew upgrade mreasonyang/taps/j-jump
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh | sh -s -- --replace

# 卸载：先移除 Shell 配置，再删除程序
jjump shell uninstall
brew uninstall mreasonyang/taps/j-jump
```

如果是用安装脚本安装的，最后在[发布压缩包](packaging/README.md#replace-recover-and-remove)（英文）中运行 `install.sh --uninstall`。你的配置、访问记录和 Key 会保留，需要时可以[手动清理](docs/CONFIGURATION-AND-HELP.md)。

## 平台支持

发布版在 macOS 15+（Apple 芯片和 Intel）以及 Linux（x86-64 和 ARM64）上进行原生测试，覆盖 Homebrew 和安装脚本两种方式，以及 Bash、Zsh、Fish。可执行文件未签名，安装程序改用 SHA-256 校验完整性。不支持 Windows、32 位系统和较旧的 macOS。详见[发布状态与平台限制](docs/RELEASE.md)（英文）。

## 文档

- [配置、帮助与恢复](docs/CONFIGURATION-AND-HELP.md)：设置、语义服务、隐私、数据管理
- [Configuration guide](docs/CONFIGURATION.md)：英文版配置与故障排查指南
- [安装脚本与发布说明](docs/RELEASE.md)（英文）
- [压缩包安装、Shell 接入与卸载](packaging/README.md)（英文）

## 参与贡献

欢迎报告问题、提出想法或提交 Pull Request。从源码构建和运行测试的方法见 [CONTRIBUTING.md](CONTRIBUTING.md)（英文）。

## 许可证

[MIT](LICENSE.md)。发布压缩包中附带全部依赖的许可证与声明。
