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

你从没输入过 “resumes” 或 “taxes”。J-Jump 会记住你去过的目录：记得名字，几个字母就能跳到；记不清名字，可选的 Jev 语义模型会按含义帮你找到，中英文混用也没问题。

## 为什么选择 J-Jump

- **按名称，即刻到达。** `j pay` 会在你真正去过的目录中找到最佳匹配并直接跳转。排序完全在本机完成，不联网。
- **按含义，交给 Jev。** 想不起目录叫什么？`ji my cv` 会请 Jev 判断你要找的是哪个目录。
- **任何语言都能用。** `ji 税务` 能找到 `taxes`，`ji machine learning experiments` 能找到 `机器学习实验`。
- **决定权始终在你。** Jev 默认关闭，开启后也只提建议，你不选就不会跳转。默认的 strict 模式下，Jev 只能看到你的查询词和候选目录名。
- **小巧、原生。** 单个 Rust 可执行文件，支持 macOS 和 Linux 上的 Bash、Zsh、Fish。不需要额外运行时或插件管理器，本地导航也不需要任何后台服务。

## 快速开始

### 1. 安装

**Homebrew**：

```sh
brew install mreasonyang/taps/j-jump && jjump shell install
```

**安装脚本**（macOS 15+ 或 Linux；x86-64 或 ARM64）：

```sh
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh | sh
```

脚本会自动选择适合你系统的版本，校验 SHA-256，然后把 `jjump`（以及等价的 `j-jump`）安装到 `~/.local/bin`。它会自动接入 Bash、Zsh 或 Fish，补齐 PATH，并备份已有启动文件。全程不需要 sudo；使用 `--no-shell` 可跳过 Shell 配置。[脚本选项](docs/RELEASE.md#download-installer)（英文）。

### 2. 打开新终端

上面的安装命令会自动完成 Shell 接入。打开新终端后即可使用 `j` 和 `ji`。
已有安装可运行 `jjump shell install`；用 `jjump shell uninstall` 撤销自动配置。
[Shell 选项、备份与手动接入](packaging/README.md)（英文）。

> `j` 已经被别的命令占用了？运行 `jjump shell install --cmd jump`，就会改为提供 `jump` 和 `jumpi`。J-Jump 不会覆盖已有的 `j`/`ji` 命令。

### 3. 开始跳转

照常使用 `cd` 即可。J-Jump 的访问记录一开始是空的，会在你每次进入目录时自动学习。第一次运行 `j` 或 `ji` 时会弹出简短的设置向导；暂时不想用 Jev 就选择仅本地模式，不需要任何 Key。之后随时可以用 `jjump setup` 修改。

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
| `ji pay` | 从匹配结果中选择；开启 Jev 后会先问 Jev |
| 输入 `j pay ` 后按 <kbd>Tab</kbd> | 把选中的目录填进命令行，按 <kbd>Enter</kbd> 才执行 |

**匹配如何排序：** 目录名完全相同优先，其次是前缀匹配、包含匹配，最后是上级目录匹配。同一档内，常去和最近去过的目录排在前面，访问权重每七天减半。想知道为什么是这个结果，运行 `jjump explain pay`。

**选择器操作：** 输入编号跳转，`n`/`p` 翻页，`v 3` 查看完整路径，直接回车或 `q` 取消。如果更喜欢模糊搜索，安装 [fzf](https://github.com/junegunn/fzf) 后设置 `export J_JUMP_PICKER=fzf`。

## Jev：按含义找目录

Jev 是可选的语义模型服务，**默认关闭**，本地导航从不依赖它。

**开启方法：** 运行 `jjump setup`，选择 Jev 并粘贴 Key。输入内容不会显示，Key 保存在系统凭证库中，不会写入任何文件。也可以在环境变量中设置 `TYPESAFE_API_KEY`，再运行 `jjump config set semantic on`。仅配置 Key 并不会开启请求。

**什么时候会问 Jev：**

- `j pay` 在本地有匹配时直接跳转，不会联系 Jev；只有本地完全没有匹配时，才可能询问 Jev。
- 开启后，`ji 查询词` 和带查询词的 <kbd>Tab</kbd> 补全会询问 Jev。不带参数的 `ji`、`--offline` 和 `J_JUMP_OFFLINE=1` 只在本地查找。
- Jev 推荐的目录会排在列表第一位并标注 `[Jev]`，仍需你亲自选择；取消不会选中任何目录。
- 默认 `consent ask`，每次请求前都会征求你的同意；`jjump config set consent always` 可跳过确认。Jev 服务可能按请求计费。
- 每次请求最多等待 10 秒。等待时按回车切换到本地候选，按 `W` 继续等待。出错、超时或 Jev 无法确定时都会停下，不会替你跳转。

### Jev 能看到什么

| `privacy` 设置 | 发送给 Jev 的内容 |
| --- | --- |
| `strict`（默认） | 查询词和候选目录名 |
| `balanced` | 另加每个目录的上级目录名、当前目录名，以及低/中/高三档访问频率 |
| `full` | 与 `balanced` 相同，但上级目录和当前目录改为完整路径 |

文件内容、Git 远程地址、环境变量、Shell 历史和凭证**永远不会**发送。目录名和查询词本身也可能敏感，所以你还可以：

- 用 `jjump preview "my cv"` 查看将要发送的完整请求，此命令不会真正发送。
- 把目录加入 `no_send`：仍可在本地查找和跳转，但不会发给 Jev；身处其中时 Jev 自动停用。
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
| `consent` | `ask`、`always` | `ask` |
| `privacy` | `strict`、`balanced`、`full` | `strict` |
| `tracking` | `on`、`off` | `on` |
| `exclude`、`no_send` | 绝对路径的 JSON 数组 | `[]` |
| `language` | `auto`、`en`、`zh` | `auto` |
| `semantic_route` | `local_first`、`force`（总是询问 Jev，仍需你选择） | `local_first` |
| `candidate_limit` | `1`–`254`，提供给 Jev 的目录分组上限 | `254` |

| 环境变量 | 作用 |
| --- | --- |
| `TYPESAFE_API_KEY` | Jev Key，优先于系统凭证库中保存的 Key |
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
jjump data clear --preview          # 访问记录和 Jev 缓存
jjump credential delete             # 删除保存的 Jev Key（预览）
```

所有删除或替换数据的命令都会先预览，加 `--apply` 才真正执行。备份必须放在私有目录（`chmod 700`）中。删除不等于安全擦除。

## 常见问题

| 问题 | 解决办法 |
| --- | --- |
| `j foo` 找不到目录 | J-Jump 只认识安装之后去过的目录。先 `cd` 进去一次，或用 `jjump explain foo` 检查。 |
| `j` 或 `ji` 已被占用 | 用 `--cmd jump` 初始化，改用 `jump` 和 `jumpi`。 |
| Bash 中按 <kbd>Tab</kbd> 没有填入选择 | 终端没有回应 Bash 的光标位置查询，命令行保持原样。改用 `ji foo`。 |
| Linux 上无法保存 Key | 系统凭证库需要 Secret Service 会话（例如 GNOME Keyring）。也可以改用 `TYPESAFE_API_KEY`。 |
| 报错提到旧格式或未知格式 | J-Jump 尚未发布 1.0，只读取当前格式的数据。原文件不会被改动，运行 `jjump doctor` 查看处理方法。 |
| 其他问题 | 运行 `jjump doctor`，它离线运行并给出下一步建议。 |

供脚本使用的退出码：`2` 输入错误，`3` 无匹配，`4` 需要选择，`5` Jev，`6` 路径，`7` 状态，`130` 已取消。

## 更新与卸载

| | Homebrew | 安装脚本 |
| --- | --- | --- |
| 更新 | `brew upgrade mreasonyang/taps/j-jump` | 带 `--replace` 重新运行安装脚本（见下方） |
| 卸载 | `brew uninstall mreasonyang/taps/j-jump` | 在[发布压缩包](packaging/README.md#replace-recover-and-remove)（英文）中运行 `install.sh --uninstall` |

```sh
curl -fsSL https://raw.githubusercontent.com/mreasonyang/j-jump/main/install.sh | sh -s -- --replace
```

卸载会保留你的配置、访问记录和已保存的 Key；如需一并删除，请先用上面的命令清理。卸载可执行文件前，运行 `jjump shell uninstall` 撤销自动配置；手动添加的 `jjump init` 行请自行删除。

## 平台支持

| 系统 | 安装脚本 | Homebrew |
| --- | --- | --- |
| macOS 15+（Apple 芯片） | ✅ | ✅ |
| macOS 15+（Intel） | ✅ | 可用 |
| Linux x86-64（静态 musl 构建） | ✅ | 可用 |
| Linux ARM64（静态 musl 构建） | ✅ | 可用 |

✅ 表示安装后的 0.0.35 发布版已在该系统上通过原生测试。其中 Apple 芯片的 Homebrew 生命周期已在独立的非默认安装目录中通过测试；“可用”表示 Formula 支持该系统，但当前发布版的这条 Homebrew 路径尚未经过原生测试。可执行文件未签名（macOS 上未经公证），安装程序改用 SHA-256 校验完整性。不支持 Windows、32 位系统和 macOS 15 以下版本。详见[发布状态与平台限制](docs/RELEASE.md)（英文）。

<details>
<summary><b>从源码构建</b></summary>

需要 Rust 1.88+（可通过 [rustup](https://rustup.rs) 安装）、C 工具链和 Python 3。SQLite 和 TLS 已内置。

```sh
git clone https://github.com/mreasonyang/j-jump.git && cd j-jump
cargo build --locked --release
python3 scripts/package.py --binary target/release/jjump --target "$(rustc -vV | sed -n 's/^host: //p')" --dist dist
cd dist && sha256sum -c j-jump-*.tar.gz.sha256     # macOS：shasum -a 256 -c ...
tar -xzf j-jump-*.tar.gz && cd j-jump-*/
./install.sh --prefix "$HOME/.local"
```

替换和卸载已有安装，请参阅[压缩包安装说明](packaging/README.md)（英文）。

</details>

## 文档

- [配置、隐私与恢复指南](docs/CONFIGURATION-AND-HELP.md)
- [安装脚本、发布与平台限制](docs/RELEASE.md)（英文）
- [压缩包安装、替换与卸载](packaging/README.md)（英文）
- [文档索引](docs/README.md)（英文）

## 参与贡献

欢迎在 [GitHub Issues](https://github.com/mreasonyang/j-jump/issues) 中报告问题或提出想法。修改代码前，请先阅读 [开发说明](AGENTS.md)（英文）并运行 `./scripts/test-product.sh`。

## 许可证

[MIT](LICENSE.md)。发布压缩包中附带全部依赖的许可证与声明。
