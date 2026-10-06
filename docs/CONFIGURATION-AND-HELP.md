# J-Jump 配置、帮助与恢复

这是与当前源码核对的用户指南；版本以根目录 [VERSION](../VERSION) 为准。公开安装渠道和平台限制见 [发布说明](RELEASE.md)。产品仅提供 Bash/Zsh/Fish CLI 和终端选择器，无 GUI、zoxide 前置依赖或自动语义跳转。

`jjump` 与 `j-jump` 完全等效，共用配置、历史和凭证；本文与程序提示默认使用 `jjump`。

## 接入与发现

公开版本 0.0.34 支持自动接入。
0.0.34 的归档安装脚本默认根据 `SHELL` 接入 Bash、Zsh 或 Fish，并补齐 PATH。完成后打开新终端即可使用。
使用 `--no-shell` 可跳过配置，`--shell bash|zsh|fish` 可指定 Shell。对于已经安装的 0.0.34，包括新版 brew 安装，可运行：

```sh
jjump shell install
jjump shell install --shell zsh --cmd jump
jjump shell install --shell zsh --rc /absolute/startup-file
jjump shell uninstall
```

接入前会备份已有启动文件，只添加带标记的配置块；重复执行不会重复写入。已有手动接入会保留。
Zsh 尊重 `ZDOTDIR`，Fish 尊重 `XDG_CONFIG_HOME`；Bash 同时处理 `.bashrc` 和登录配置文件。
自定义 `--rc` 只修改指定文件。撤销只删除未被改动的自动配置块，其他设置和备份保留。
发现已有命令冲突时，可用 `--cmd jump`；复杂启动配置中的动态冲突仍由 Shell 初始化时检查。
自动接入不会执行已有启动脚本，也不会创建产品配置、读取凭证或开启 Jev。

对于需要手动接入的情况，把 binary 所在目录加入 PATH，再选择当前 Shell 对应命令：

```sh
# Zsh
eval "$(jjump init zsh)"
# Bash
eval "$(jjump init bash)"
# Fish
jjump init fish | source
```

`init` 不修改 shellrc，不联网。自行将对应命令放入启动文件时不要重复接入。冲突时可用 `init zsh --cmd jump` 生成 jump/jumpi；不要覆盖已有命令。`j --help`/`ji --help` 显示当前命令名用法，不切换目录。

接入后首次在终端执行 `j` 或 `ji`，未配置时会直接进入向导，无需另输配置命令。保存后继续刚才的导航，取消则不导航、不保存，下次执行仍可配置。已有配置不重复引导；后续修改可用 `jjump setup`。本地功能不需要 key。先用普通 cd 访问目录，在提示符出现时积累历史。`j` 回 HOME、`j -` 回原生上一目录；直接路径即使配置或历史损坏也可继续使用。第一次无上一目录时原生 Shell 返回错误；先完成一次 cd 再重试。

帮助、补全、init、提示符记录，以及输入或诊断输出被重定向的命令不会触发自动向导。更换 `J_JUMP_CONFIG` / `J_JUMP_HOME` 后按所选配置判断是否首次使用。

## 可返回的配置草稿

首次 setup 是短向导，显示初始历史和 Shell 尚未验证状态。Enter 保留值，b 返回，q/EOF/Ctrl-C 放弃草稿；误输入在当前题重问。回答完最后一步后自动保存，不再询问一次确认。再次 setup 提供语义、记录、路径/高级、语言、重置、API Key 设置、离线就绪检查菜单，选择一项即可修改，最后选择一次“保存”即完成。并发配置变化会拒绝覆盖，需重新载入。

路径编辑逐条输入绝对路径，Enter 完成本组、clear 清空本组草稿。“不记录、不搜索的文件夹”：该文件夹及子文件夹不进入访问记录和本地搜索，直接输入路径仍可跳转。

“不向 Jev 发送目录信息的文件夹”：仍可在本地查找和跳转，但名称、路径不会发给 Jev，子文件夹也适用；当前位于其中时不调用 Jev。例如添加 `/work/保密客户`，让这个客户的目录信息只留在本机。J-Jump 始终不会上传文件内容。完成向导前仍可退出。恢复默认保留已有停记/排除策略、访问库、缓存和凭证。

## 保存不等于可以联网

语义开关、服务商、许可、发送字段和凭证分别配置。服务商默认为 `jev`，可选 `clef-flash` 和本机 `tev1`。默认 off、ask、strict；没有请求次数设置或累计上限。选择 Jev 后会直接进入 API Key 设置；选择 Clef-Flash 则先填写 Cloudflare Account ID，再设置 API Token：隐藏输入新 Key、保留已配置来源、使用已检测到的环境变量，或跳过。Key 仅在内存草稿中，完成向导时写入系统凭证库；取消或恢复默认会丢弃待保存 Key。凭证库写入失败会保留草稿，可在向导内重试或编辑。也可以暂不配凭证，页面会列出阻断原因；不会为了验证而自动请求。

- ask：每次请求前问；always：允许按所选隐私规则联网，不再逐次询问。
- strict：查询及候选名；balanced：增加有限上下文；full：允许路径。名称/查询本身也可能敏感。
- Jev 使用 TYPESAFE_API_KEY；Cloudflare 使用 CLOUDFLARE_AUTH_TOKEN（为空或缺失时使用 CLOUDFLARE_API_TOKEN），各自优先于对应系统凭证。CLOUDFLARE_ACCOUNT_ID 优先于保存的 cloudflare_account_id，必须是 32 位十六进制值。切换服务商会清除未保存的凭证草稿，但保留已保存的两家凭证。
- `jjump credential status/set/delete` 只作用于所选服务商；在向导中输入或更换 Key，隐藏输入到 OS 存储，无明文 fallback，不需要额外执行凭证命令。独立的 `jjump credential set` 仍可用于专门管理凭证。状态命令不解锁/读取 OS key，因此“系统存储已配置”不等于已确认 key 存在或有效。
- `--offline`/`J_JUMP_OFFLINE=1` 限制当前命令，不会改永久配置；凭证存在不代表联网许可。

命令行可写键和值见 `jjump config set --help`。例如：

```sh
jjump config set semantic off
jjump config set tracking off
jjump config set privacy strict
jjump config set language zh
```

## 选择器与等待

普通 j 本地有有效词法匹配就直接跳、零模型请求；只有真正无匹配才可能 Jev。带查询 ji/Space-Tab 可请求 Jev；裸 ji/空补全为本地。强制语义仅显式 `--force-semantic` 或 `semantic_route=force` 开启，默认 local_first。候选为已访问库存中的作用域/词法候选，不爬盘、不读源码。

云端请求遵循系统/环境代理；本机 Tev1 只直连 loopback，单次交互总期限10秒、连接上限3秒。等待约0.9秒后 Enter 进入本地 picker，W 继续同一时限；Enter 本身不会选择或 cd，迟到结果不会接管选择。超时、服务失败、响应非法或无可靠建议时停止，不选目录、不换传输或自动打开本地选择器。Jev 仅通过 adapter 请求；可执行 `jjump --offline query --interactive` 主动浏览本地。

默认使用内置编号模式。显式设置 `J_JUMP_PICKER=fzf` 才启用本地搜索/上下键/翻页；fzf 缺失、执行失败或终端不支持时直接报错。可显式改用 `J_JUMP_PICKER=numbered`。编号模式 n 下一页、p 上一页、v N 查看完整路径、编号选择、空输入/q取消；无效编号可重输。40列下保留路径尾部，完整输出路径和身份校验不受缩写影响。显式本地查询没有词法匹配时说明没有匹配；裸 `ji` 浏览全部允许的历史目录。启用 Jev 时的语义 shortlist 可以包含非词法候选，与本地查询不同。

Space-Tab 的选择只写入可编辑命令行，Enter 才执行。Bash 需要终端标准设备状态回复；不响应的终端保留原行，取消后可改用 ji。

## 可读状态与恢复

`jjump config show` 显示保存值、生效值、来源、脱敏路径及下一步；`doctor` 离线检查配置与库，父 Shell 是否接入仍需本人在终端执行一次 `j -- /`、再 `j -` 验证。`--json` 保留版本化字段，供自动化。**config show --json 保留原始排除路径；分享前检查。** doctor JSON 仍不输出私有路径/密钥。

```sh
jjump doctor
jjump config recover             # 预览，不修改
jjump config recover --apply     # 恢复配置，语义/记录都关闭
jjump config reset --preview     # 预览恢复默认
jjump config reset --apply       # 恢复默认，保留记录/排除策略
```

recover 不修复历史库，也无法替换被安全策略拒绝的符号链接/非私有配置文件；doctor 会分别给原因和适用操作。损坏配置恢复不复制可能含秘密的原文，访问库/凭证保留；重新启用记录前先补回排除规则。历史备份/恢复和清理的 preview/apply 不等于安全擦除。

## 语言和范围

默认从 LC_ALL、LC_MESSAGES、LANG 选择中/英文，未知 locale 用英文。`language=auto/en/zh` 为持久偏好，J_JUMP_LANG 覆盖它。核心引导/帮助/诊断与常见错误提供双语信息；技术键名、命令、稳定错误码、JSON和查询不翻译。Jev等待文案保持英文。NO_COLOR 下不依赖颜色；编号界面是逐行文本。

开发期只接受当前完整格式：配置 schema 3、访问库 schema 4、备份 schema 3、语义状态 schema 4。配置 schema 3 包含默认值字段 provider、cloudflare_account_id、cloudflare_credential，以及经过注册表校验的 providers 设置组，原有完整 schema 3 配置可直接读取，默认仍为 Jev；不重写用户配置。旧格式被拒绝且保持原样；不自动转换/迁移，也不提供旧版降级路径。需要独立文件状态时显式指定绝对路径 J_JUMP_HOME；它只重定向配置、访问库和缓存，不隔离系统凭证。所有文件 profile 按服务商共用当前 OS 用户的凭证项：Jev 为 `j-jump.jev` / `typesafe-api-key`，Cloudflare 为 `j-jump.cloudflare` / `workers-ai-api-token`；凭证测试还须隔离 OS 服务。Linux 使用 XDG 路径，macOS 使用 Application Support/Caches；配置优先级为 `--config`、J_JUMP_CONFIG、默认路径，不读取工作目录中的项目配置。

系统凭证、锁定/拒绝提示、完整平台覆盖、真实 Jev 质量及新手理解均须分别验收。见 [平台与发布范围](RELEASE.md)，不把系统凭证夹具或某个版本的测试泛化为全部环境通过。

## 常用默认值和数据管理

| 设置 | 默认值 | 含义 |
| --- | --- | --- |
| semantic / consent / privacy | off / ask / strict | 关闭语义；启用后仍按许可与披露规则工作 |
| tracking | on | 记录提示符处的实际目录变化，可暂停 |
| credential | environment | 环境来源优先；setup 可选择系统存储 |
| candidate_limit |254| 按披露名称/上下文分组的上限，可设1–254；单问题、64KiB 内按优先顺序截取；不截断本地词法列表 |
| semantic_route | local_first | 普通 j 优先本地；force 为显式选项 |
| proxy | system | 云端使用系统/环境网络路径；本机模型强制 loopback 直连 |
| language | auto | 跟随 locale，可选 en/zh |

```sh
jjump history list
jjump history forget --preview -- /work/old-project
jjump history clear --preview
jjump history backup /absolute/private-backup/visits.json
jjump history restore /absolute/private-backup/visits.json
jjump cache clear --preview
jjump data clear --preview
jjump credential delete              # 仅预览
```

删除操作加 `--apply` 才执行；restore 默认只验证，加 `--apply` 才替换当前访问库。备份父目录须私有，恢复仅支持当前格式；无效备份不会替换现有访问库。访问清理会使旧快照/缓存绑定失效，之后真实访问仍可重新记录。cache clear 不删除访问记录或凭证，也没有需要保留的累计请求计数。data clear 不删除 Shell/第三方历史、备份或凭证。删除 OS key 不会撤销服务端账户密钥或修改环境变量。

`jjump explain QUERY` 显示排序及实际选择；`--json` 输出机器诊断。编号选择器可输入文字筛选、编号选择、v N 查看路径及 n/p 翻页。有效 Jev 建议的所有本地目录先列出，仍须明确选择；取消静默返回130。`history prune` 先预览，`--apply` 删除已确认缺失的访问统计，不删除目录。


## 本机模型与当前语义状态

`jjump setup` 选择 `tev1` 后填写 Ollama 地址，跳过 Key 步骤。命令行可用 `config set ollama_url http://127.0.0.1:11434`。
地址只允许本机 `127.0.0.1` 或 `[::1]` 的 HTTP origin，不允许远程主机、URL 凭据、路径、查询或片段。
Ollama 至少 0.35，模型固定 `tev1:4b`，需兼容 GGUF；用户负责安装、下载和启动。

`jjump provider-check --json` 是本机模型的显式连接检查：两条元数据请求加一条合成推理，最多 10 秒。
它不发送访问历史，不测量目录质量；默认 doctor 与 setup 不调用它。检查失败保留设置，可按错误修复服务/模型后重试。
`credential status` 显示来源 none；set/delete 对不需要凭据的 Provider 拒绝操作，不接触其他服务的凭据。

candidate_limit 的保存值仍为 1–254；实际 Tev1 请求最多 23 个目录组加 none，并受保守的 1,600 字节序列化上下文预算约束。
预算为模型模板和评分位置留出余量，超限减少候选；查询本身不会被截断。preview 展示实际发送结果。
Tev1 不持久缓存答案，避免模型标签换权重后使用旧结果；云端缓存仍绑定服务、端点、模型、认证身份和请求快照。
熔断按服务身份隔离，同一服务的不同查询共享故障计数。

当前语义状态使用 `semantic-drivers.db`（schema 4）。此前 `semantic-cache.db` 保留原样且不读取、迁移或复用，
当前 cache/data clear 只清理当前状态文件；清理不等于安全擦除。访问库和系统凭据不迁移。
