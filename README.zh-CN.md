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

## 为什么选择 J-Jump

- **按名称，即刻到达。** `j pay` 会在你真正去过的目录中找到最佳匹配并直接跳转。排序完全在本机完成，不联网。
- **按含义，模型由你选。** 想不起目录叫什么？`ji my cv` 会请语义模型判断你要找的是哪个目录：可以用云端的 Jev 或 Cloudflare Clef-Flash，也可以用完全运行在你电脑上的 Tev1。
- **任何语言都能用。** `ji 税务` 能找到 `taxes`，`ji machine learning experiments` 能找到 `机器学习实验`。
- **决定权始终在你。** 语义功能默认关闭，开启后也只提建议，你不选就不会跳转。默认的 strict 模式下，模型只能看到你的查询词和候选目录名。
- **小巧、原生。** 单个 Rust 可执行文件，支持 macOS 和 Linux 上的 Bash、Zsh、Fish。不需要额外运行时或插件管理器，本地导航也不需要任何后台服务。

## 快速开始

**1. 安装。** 下面任一命令都会同时接入你的 Shell。

```sh
# Homebrew
brew install mreasonyang/taps/j-jump && jjump shell install

# 或使用安装脚本（macOS 15+ 或 Linux，x86-64 或 ARM64）
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh | sh
```

安装脚本会自动选择适合你系统的版本，校验 SHA-256，然后把 `jjump`（以及等价的 `j-jump`）安装到 `~/.local/bin`，全程不需要 sudo。Shell 接入会识别 Bash、Zsh 或 Fish，先备份启动文件，再添加一段带明确标记的配置（包括 `PATH`）。不想自动接入，可给脚本加上 `--no-shell`。[脚本选项](docs/RELEASE.md#download-installer)（英文）· [Shell 选项](packaging/README.md)（英文）

**2. 打开一个新终端。** 照常使用 `cd` 即可：J-Jump 的访问记录一开始是空的，会在你每次进入目录时自动学习。

**3. 运行 `j` 或 `ji`。** 第一次运行会弹出简短的设置向导。选择 `off` 就只用本地功能，不需要任何 Key；也可以现在就选一个语义服务。之后随时可以用 `jjump setup` 修改。

> `j` 已经被别的命令占用了？用 `jjump shell install --cmd jump`，就会改为提供 `jump` 和 `jumpi`。J-Jump 不会覆盖已有的 `j`/`ji` 命令。

## 日常使用

| 命令 | 作用 |
| --- | --- |
| `j pay` | 跳到与 `pay` 最匹配的已访问目录 |
| `j api server` | 多个关键词按顺序匹配路径 |
| `j api /` | 只在当前目录之下查找 |
| `j` | 回到主目录 |
| `j -` | 回到上一个目录 |
| `j ../dir`、`j -- 'my folder'` | 也可以直接写路径 |
| `ji` | 用编号列表浏览全部访问记录 |
| `ji pay` | 从匹配结果中选择；开启语义功能后会先询问语义服务 |
| 输入 `j pay ` 后按 <kbd>Tab</kbd> | 把选中的目录填进命令行，按 <kbd>Enter</kbd> 才执行 |

**匹配如何排序：** 目录名完全相同优先，其次是前缀匹配、包含匹配，最后是上级目录匹配。同一档内，常去和最近去过的目录排在前面，访问权重每七天减半。运行 `jjump explain pay` 可以查看排序依据。

**选择器操作：** 输入编号跳转，`n`/`p` 翻页，`v 3` 查看完整路径，直接回车或 `q` 取消。如果更喜欢模糊搜索，安装 [fzf](https://github.com/junegunn/fzf) 后设置 `export J_JUMP_PICKER=fzf`。

## 按含义找目录

语义功能**默认关闭**，本地导航从不依赖它。从下面选一个语义服务：

| 服务 | 运行位置 | 你需要准备 | 可用版本 |
| --- | --- | --- | --- |
| `jev`（默认） | 云端 | Jev API Key | 发布版 0.0.38 |
| `clef-flash` | 云端（[Cloudflare Workers AI](https://developers.cloudflare.com/workers-ai/models/clef-flash/)） | Cloudflare Account ID 和 Workers AI API Token | 发布版 0.0.38 |
| `tev1` | 你的电脑，通过 [Ollama](https://ollama.com/download) | Ollama 0.35+ 和约 4.5 GB 的模型；无需 Key | 源码 0.0.40，尚未发布 |

最简单的配置方式是运行 `jjump setup`：它会引导你选择服务、填写 Key 或本地模型，并开启语义功能。Key 隐藏输入，保存在系统凭证库中，不会写入任何文件；每个服务各用一个独立的凭证项。

**工作方式：**

- `j pay` 在本地有匹配时直接跳转，不会联系任何服务；只有本地完全没有匹配时，才可能询问。
- `ji 查询词` 和带查询词的 <kbd>Tab</kbd> 补全会询问语义服务。不带参数的 `ji`、`--offline` 和 `J_JUMP_OFFLINE=1` 只在本地查找。
- 建议的目录排在列表第一位，并标注服务名称；仍需你亲自选择，取消不会选中任何目录。
- 默认 `consent ask`，每次请求前都会征求你的同意；`jjump config set consent always` 可跳过确认。云端服务可能按请求计费。
- 每次请求最多等待 10 秒。等待时按回车切换到本地候选，按 `W` 继续等待。出错、超时或模型无法确定时都会停下，不会替你跳转；J-Jump 也不会自动改用其他服务。

<details>
<summary><b>不用向导配置 Jev</b></summary>

```sh
jjump config set provider jev
export TYPESAFE_API_KEY="your-jev-key"     # 或保存到凭证库：jjump credential set
jjump config set semantic on
```

环境变量优先于已保存的 Key。只有 Key 并不会开启请求。

</details>

<details>
<summary><b>不用向导配置 Cloudflare Clef-Flash</b></summary>

```sh
jjump config set provider clef-flash
export CLOUDFLARE_ACCOUNT_ID="your-32-character-account-id"
export CLOUDFLARE_AUTH_TOKEN="your-workers-ai-api-token"
jjump config set semantic on
```

- `CLOUDFLARE_AUTH_TOKEN` 未设置或为空时，可以用 `CLOUDFLARE_API_TOKEN` 代替。
- `CLOUDFLARE_ACCOUNT_ID` 优先于已保存的 `cloudflare_account_id`。值无效时会阻止请求，改正或删除该变量即可。
- `jjump doctor` 只在本地检查这些值的格式，不会验证 API 权限，也不会发出请求。

</details>

<details>
<summary><b>本地 Tev1 4B（源码 0.0.40）</b></summary>

Tev1 通过 Ollama 在你的电脑上运行。J-Jump 不会安装或启动 Ollama，也不会自动下载模型。

```sh
# 1. 安装 Ollama 0.35+ 并打开应用（或保持 `ollama serve` 运行），然后：
ollama pull tev1:4b-q8_0
# 2. 在向导中选择 tev1，并在连接检查一步输入 `check` 进行测试。
jjump setup
```

也可以直接配置：

```sh
jjump config set provider tev1
jjump config set ollama_url http://127.0.0.1:11434
jjump config set ollama_model tev1:4b-q8_0
jjump provider-check --models      # 列出已安装的兼容模型，不加载模型
jjump provider-check               # 加载模型并发送一条合成测试请求
jjump config set semantic on
```

- 支持的 [GGUF 标签](https://ollama.com/library/tev1/tags)：`tev1:4b-q8_0`（推荐）、`tev1:4b-q4_K_M`、`tev1:4b-bf16`，以及已安装的 `tev1:4b`。MLX/Safetensors 版本无法使用。
- 只连接本机回环地址，不经过代理。如需换端口，用 `env OLLAMA_HOST=127.0.0.1:11439 ollama serve` 启动 Ollama，并在 J-Jump 中填写相同地址。
- 10 秒时限包含模型加载时间，冷启动后的第一次请求可能需要重试。每次请求后，Ollama 会把模型在内存中保留五分钟。
- Tev1 每次最多看到 23 组目录，你要找的目录不一定在其中。它的回答不会跨次缓存。
- 向导只有在你输入 `list` 或 `check` 时才会联系 Ollama；`doctor`、`preview` 和状态命令都不联网。

</details>

### 语义服务能看到什么

| `privacy` 设置 | 发送给语义服务的内容 |
| --- | --- |
| `strict`（默认） | 查询词和候选目录名 |
| `balanced` | 另加每个目录的上级目录名、当前目录名，以及低/中/高三档访问频率 |
| `full` | 与 `balanced` 相同，但上级目录和当前目录改为完整路径 |

文件内容、Git 远程地址、环境变量、Shell 历史和凭证**永远不会**发送，本地 Tev1 也遵循同样的规则。目录名和查询词本身也可能敏感，所以你还可以：

- 用 `jjump preview "my cv"` 查看将要发送的完整请求，此命令不会真正发送。
- 把目录加入 `no_send`：仍可在本地查找和跳转，但不会被发送；身处其中时语义请求自动停用。
- 把目录加入 `exclude`：完全不记录、不搜索。

```sh
jjump config set no_send '["/work/private-client"]'
jjump config set exclude '["/work/scratch"]'
```

## 配置

可以用交互式的 `jjump setup`，也可以用 `jjump config set 键 值`。`jjump config show` 显示当前配置，`jjump doctor` 离线检查并给出下一步建议。

| 键 | 可选值 | 默认值 |
| --- | --- | --- |
| `semantic` | `on`、`off` | `off` |
| `provider` | `jev`、`clef-flash`、`tev1` | `jev` |
| `consent` | `ask`、`always` | `ask` |
| `privacy` | `strict`、`balanced`、`full` | `strict` |
| `tracking` | `on`、`off` | `on` |
| `exclude`、`no_send` | 绝对路径的 JSON 数组 | `[]` |
| `language` | `auto`、`en`、`zh` | `auto` |
| `semantic_route` | `local_first`、`force`（总是询问语义服务，仍需你选择） | `local_first` |
| `candidate_limit` | `1`–`254`，提供给云端服务的目录分组上限 | `254` |
| `cloudflare_account_id` | 32 位十六进制字符 | 空 |
| `ollama_url` | 本机回环 HTTP 地址 | `http://127.0.0.1:11434` |
| `ollama_model` | 受支持的 Tev1 4B 标签 | `tev1:4b-q8_0` |

| 环境变量 | 作用 |
| --- | --- |
| `TYPESAFE_API_KEY` | Jev Key，优先于已保存的 Jev Key |
| `CLOUDFLARE_AUTH_TOKEN`、`CLOUDFLARE_API_TOKEN` | Workers AI Token，优先于已保存的 Cloudflare Token |
| `CLOUDFLARE_ACCOUNT_ID` | 优先于已保存的 Cloudflare Account ID |
| `J_JUMP_PICKER` | `fzf` 或 `numbered`（默认） |
| `J_JUMP_OFFLINE=1` | 本次命令只在本地查找 |
| `J_JUMP_LANG` | 界面语言：`en` 或 `zh` |
| `J_JUMP_CONFIG` | 使用另一个配置文件（`--config` 优先） |
| `J_JUMP_HOME` | 把配置、访问记录和缓存放到另一个绝对路径下（系统凭证项仍然共用） |

macOS 上的文件位于 `~/Library/Application Support/j-jump` 和 `~/Library/Caches/j-jump`；Linux 上位于 XDG 目录 `~/.config/j-jump`、`~/.local/share/j-jump` 和 `~/.cache/j-jump`。

## 访问记录与数据

```sh
jjump history list
jjump history forget --preview -- /work/old-project
jjump history prune                 # 预览已不存在的目录记录
jjump config set tracking off       # 暂停记录
mkdir -m 700 ~/jjump-backup && jjump history backup ~/jjump-backup/visits.json
jjump history restore ~/jjump-backup/visits.json    # 只做校验；加 --apply 才替换
jjump history clear --preview
jjump data clear --preview          # 访问记录和语义回答缓存
jjump credential delete             # 删除当前服务已保存的 Key（预览）
```

所有删除或替换数据的命令都会先预览，加 `--apply` 才真正执行。备份必须放在私有目录（`chmod 700`）中。删除不等于安全擦除。

## 常见问题

| 问题 | 解决办法 |
| --- | --- |
| 安装后提示 `j: command not found` | 打开一个新终端。如果仍然不行，运行 `jjump shell install` 并按提示处理。 |
| `j foo` 找不到目录 | J-Jump 只认识安装之后去过的目录。先 `cd` 进去一次，或用 `jjump explain foo` 检查。 |
| `j` 或 `ji` 已被占用 | 用 `jjump shell install --cmd jump`，改用 `jump` 和 `jumpi`。 |
| Bash 中按 <kbd>Tab</kbd> 没有填入选择 | 终端没有回应 Bash 的光标位置查询，命令行保持原样。改用 `ji foo`。 |
| Linux 上无法保存 Key | 系统凭证库需要 Secret Service 会话（例如 GNOME Keyring）。也可以改用对应服务的环境变量。 |
| Tev1 没有回应 | 确认 Ollama 正在运行且模型已下载，然后运行 `jjump provider-check`。冷启动时可能需要重试。 |
| 报错提到旧格式或未知格式 | J-Jump 尚未发布 1.0，只读取当前格式的数据。原文件不会被改动，运行 `jjump doctor` 查看处理方法。 |
| 其他问题 | 运行 `jjump doctor`，它离线运行并给出下一步建议。 |

供脚本使用的退出码：`2` 输入错误，`3` 无匹配，`4` 需要选择，`5` 语义服务，`6` 路径，`7` 状态，`130` 已取消。

## 更新与卸载

```sh
# 更新
brew upgrade mreasonyang/taps/j-jump                                                         # Homebrew
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh | sh -s -- --replace   # 安装脚本

# 卸载：先移除 Shell 配置，再删除程序
jjump shell uninstall
brew uninstall mreasonyang/taps/j-jump                                                       # Homebrew
```

用安装脚本安装的，先运行 `jjump shell uninstall`，再在[发布压缩包](packaging/README.md#replace-recover-and-remove)（英文）中运行 `install.sh --uninstall`。卸载会保留你的配置、访问记录和已保存的 Key；如需一并删除，请先用上面的命令清理。`jjump shell uninstall` 只移除未被改动过的 J-Jump 配置段；你手动添加的 `jjump init` 行需要自己删除。

## 平台支持

| 系统 | 安装脚本 | Homebrew |
| --- | --- | --- |
| macOS 15+（Apple 芯片） | ✅ | ✅ |
| macOS 15+（Intel） | ✅ | ✅ |
| Linux x86-64（静态 musl 构建） | ✅ | ✅ |
| Linux ARM64（静态 musl 构建） | ✅ | ✅ |

发布版 0.0.38 已在以上四种系统上通过两种安装方式的原生测试，包括 Bash、Zsh 和 Fish 的接入。可执行文件未签名（macOS 上未经公证），安装程序改用 SHA-256 校验完整性。不支持 Windows、32 位系统和 macOS 15 以下版本。详见[发布状态与平台限制](docs/RELEASE.md)（英文）。

<details>
<summary><b>从源码构建</b></summary>

需要 Rust 1.88+（可通过 [rustup](https://rustup.rs) 安装）、C 工具链和 Python 3。SQLite 和 TLS 已内置。在下一个版本发布前，想试用 Tev1 需要从源码构建。

```sh
git clone https://github.com/mreasonyang/j-jump.git && cd j-jump
cargo build --locked --release
python3 scripts/package.py --binary target/release/jjump --target "$(rustc -vV | sed -n 's/^host: //p')" --dist dist
cd dist && sha256sum -c j-jump-*.tar.gz.sha256     # macOS：shasum -a 256 -c ...
tar -xzf j-jump-*.tar.gz && cd j-jump-*/
./install.sh --prefix "$HOME/.local"
```

压缩包安装程序同样会接入 Shell（加 `--no-shell` 可跳过）。替换和卸载已有安装，请参阅[压缩包安装说明](packaging/README.md)（英文）。

</details>

## 文档

- [配置、隐私与恢复指南](docs/CONFIGURATION-AND-HELP.md)
- [安装脚本、发布与平台限制](docs/RELEASE.md)（英文）
- [压缩包安装、Shell 接入与卸载](packaging/README.md)（英文）
- [文档索引](docs/README.md)（英文）

## 参与贡献

欢迎在 [GitHub Issues](https://github.com/mreasonyang/j-jump/issues) 中报告问题或提出想法。修改代码前，请先阅读[开发说明](AGENTS.md)（英文）并运行 `./scripts/test-product.sh`。

## 许可证

[MIT](LICENSE.md)。发布压缩包中附带全部依赖的许可证与声明。
