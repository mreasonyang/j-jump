# Configuration and reference

Everything you can change in J-Jump, plus provider setup, privacy controls, data commands and troubleshooting.
For installation and a quick tour, start with the [README](../README.md). 中文用户请参阅[配置、帮助与恢复](CONFIGURATION-AND-HELP.md).

## Changing settings

- `jjump setup` is an interactive wizard. Enter keeps a value, `b` goes back and `q` leaves without saving.
- `jjump config set KEY VALUE` changes one setting; `jjump config set --help` lists them.
- `jjump config show` prints current values and where they come from.
- `jjump doctor` runs offline checks and suggests next steps.

| Key | Values | Default |
| --- | --- | --- |
| `semantic` | `on`, `off` | `off` |
| `provider` | `jev`, `clef-flash`, `tev1`, `openai` | `jev` |
| `consent` | `ask` (confirm every request), `always` | `ask` |
| `privacy` | `strict`, `balanced`, `full` (see [below](#what-a-provider-can-see)) | `strict` |
| `tracking` | `on`, `off` (pause recording visits) | `on` |
| `exclude`, `no_send` | JSON array of absolute paths | `[]` |
| `language` | `auto`, `en`, `zh` | `auto` |
| `semantic_route` | `local_first`, `force` (always ask the provider; you still choose) | `local_first` |
| `candidate_limit` | `1`–`254` folder groups offered to a cloud provider | `254` |
| `cloudflare_account_id` | 32 hexadecimal characters | empty |
| `ollama_url` | Loopback HTTP address | `http://127.0.0.1:11434` |
| `ollama_model` | A supported Tev1 4B tag | `tev1:4b-q8_0` |

API keys never go in the config file; enter them in `jjump setup` or use environment variables.

| Environment variable | Effect |
| --- | --- |
| `OPENAI_API_KEY` | OpenAI key; overrides the stored OpenAI key |
| `TYPESAFE_API_KEY` | Jev key; overrides the stored Jev key |
| `CLOUDFLARE_AUTH_TOKEN` | Workers AI token; overrides the stored Cloudflare token |
| `CLOUDFLARE_API_TOKEN` | Used when `CLOUDFLARE_AUTH_TOKEN` is unset or empty |
| `CLOUDFLARE_ACCOUNT_ID` | Overrides the saved Cloudflare Account ID |
| `J_JUMP_PICKER` | `numbered` (default) or `fzf` |
| `J_JUMP_OFFLINE=1` | Keep commands local, like `--offline` |
| `J_JUMP_LANG` | `en` or `zh` messages; overrides `language` |
| `J_JUMP_CONFIG` | Use another config file; `--config` takes priority |
| `J_JUMP_HOME` | Keep config, history and cache under another absolute folder. OS credential entries are still shared. |

| | Config and history | Cache |
| --- | --- | --- |
| macOS | `~/Library/Application Support/j-jump` | `~/Library/Caches/j-jump` |
| Linux | `$XDG_CONFIG_HOME/j-jump`, `$XDG_DATA_HOME/j-jump` (default `~/.config`, `~/.local/share`) | `$XDG_CACHE_HOME/j-jump` (default `~/.cache`) |

## Shell integration

The installers run `jjump shell install` for you. You can also run it yourself:

```sh
jjump shell install                         # detect your login shell from $SHELL
jjump shell install --shell zsh --cmd jump  # use jump/jumpi instead of j/ji
jjump shell install --shell zsh --rc /absolute/path/to/startup-file
jjump shell uninstall                       # remove the managed block again
```

It backs up the startup file next to it as `FILE.j-jump-backup-*`, then adds one marked block that puts `jjump` on
`PATH` and activates `j`/`ji`. Running it again changes nothing. `jjump shell uninstall` removes the block only if you
haven't edited it. To manage your dotfiles yourself, add the line instead:

```sh
eval "$(jjump init bash)"    # ~/.bashrc
eval "$(jjump init zsh)"     # ~/.zshrc
jjump init fish | source     # ~/.config/fish/config.fish
```

More details: [archive installation and shell integration](../packaging/README.md).

## Picker and completion

- **Numbered picker** (default): type a number to jump, `n`/`p` to change page, `v 3` to see a full path, Enter or `q`
  to cancel. An invalid number asks again.
- **fzf**: set `J_JUMP_PICKER=fzf` to filter with [fzf](https://github.com/junegunn/fzf). If fzf is missing or fails,
  J-Jump reports an error instead of switching pickers. Your fzf defaults are ignored, and preview/execute are disabled.
- **<kbd>Tab</kbd> completion**: type `j pay ` (with the space) and press <kbd>Tab</kbd> to put a chosen folder on the
  command line; nothing runs until you press <kbd>Enter</kbd>. Bash needs the terminal to answer a cursor-position query;
  if it doesn't, the line is left unchanged.

## Semantic providers

Semantic help is off by default, and local navigation never needs it. `j` uses a local match when there is one and only
asks a provider when nothing matches. `ji QUERY` and <kbd>Tab</kbd> with a query ask the provider when semantic help is
on. Every suggestion still needs your choice, and J-Jump never falls back to another provider on its own.

A request waits up to 10 seconds. While waiting, press Enter to switch to local choices or `W` to keep waiting. Errors,
timeouts and "not sure" answers stop without moving you. A request already sent may still finish on the server after
you cancel.

### Jev (TypeSafe AI)

[Jev](https://docs.typesafe.ai/introduction) is TypeSafe AI's decision model. Create an API key in the
[TypeSafe console](https://console.typesafe.ai/keys), then run `jjump setup`, choose `jev` and paste the key. Or:

```sh
jjump config set provider jev
export TYPESAFE_API_KEY="your-jev-key"     # or store it: jjump credential set
jjump config set semantic on
```

### Cloudflare Clef-Flash

Uses the [Workers AI REST API](https://developers.cloudflare.com/workers-ai/models/clef-flash/) with your Cloudflare
Account ID and a Workers AI API token. Choose `clef-flash` in `jjump setup`, or:

```sh
jjump config set provider clef-flash
export CLOUDFLARE_ACCOUNT_ID="your-32-character-account-id"
export CLOUDFLARE_AUTH_TOKEN="your-workers-ai-api-token"
jjump config set semantic on
```

An invalid `CLOUDFLARE_ACCOUNT_ID` blocks requests until you fix or unset it. `jjump doctor` checks the format of these
values locally; it doesn't test API permissions.

### OpenAI Decisions

Requires J-Jump 0.0.42 or later. Uses the official [Decisions API](https://developers.openai.com/api/docs/guides/decisions)
at `https://api.openai.com/v1/decisions` with `gpt-6-luna`. Choose `openai` in `jjump setup` and enter your OpenAI API key,
or configure it directly:

```sh
jjump config set provider openai
export OPENAI_API_KEY="your-openai-api-key"  # or store it: jjump credential set
jjump config set semantic on
```

`jjump preview "my cv"` shows the exact `input`, named choice question and candidate IDs without sending them.
The API's `none` choice, refusal, tied probabilities or weak evidence produce no suggestion. A valid suggestion still
requires your explicit selection. Setup and doctor check configuration locally; they do not test account access.

The OpenAI key uses the separate OS entry `j-jump.openai` / `openai-api-key`. Its source is saved under
`providers.openai.credential` (`environment` or `system`); the config stores no key. Switching providers retains saved
credential sources and clears unsaved key drafts.

### Tev1 4B on your computer

Requires J-Jump 0.0.40 or later (`jjump --version`); the current [release](RELEASE.md) includes support.
Tev1 runs locally through [Ollama](https://ollama.com/download) 0.35 or later. J-Jump never installs or starts Ollama
and never downloads models.

```sh
ollama pull tev1:4b-q8_0           # about 4.5 GB
jjump setup                        # choose tev1; type `check` at the connection step
```

Or configure it directly:

```sh
jjump config set provider tev1
jjump config set ollama_url http://127.0.0.1:11434
jjump config set ollama_model tev1:4b-q8_0
jjump provider-check --models      # list installed compatible models without loading one
jjump provider-check               # load the model and send one synthetic test request
jjump config set semantic on
```

- Supported [GGUF tags](https://ollama.com/library/tev1/tags): `tev1:4b-q8_0` (recommended), `tev1:4b-q4_K_M`,
  `tev1:4b-bf16` and existing `tev1:4b` installs. MLX/Safetensors builds don't work.
- Only loopback addresses are used, with no proxy. For another port, start Ollama with
  `env OLLAMA_HOST=127.0.0.1:11439 ollama serve` and set the same URL in J-Jump.
- The 10-second deadline includes loading the model, so the first request after a cold start may need a retry. Ollama
  keeps the model in memory for five minutes after each request.
- Tev1 sees at most 23 folder groups per question, so the folder you want may not be among them. Answers aren't cached
  between runs.
- Setup only contacts Ollama when you type `list` or `check`; `doctor`, `preview` and status stay offline.

### Keys

Keys typed into `jjump setup` are hidden and saved to the OS credential store when setup finishes; each provider has its
own entry. On Linux this needs a Secret Service session, such as GNOME Keyring; otherwise use the environment variable.
Environment variables take priority over stored keys, and a key alone never turns requests on.

```sh
jjump credential status    # doesn't unlock or show the key
jjump credential set
jjump credential delete    # preview; add --apply to remove J-Jump's entry
```

Deleting the stored key doesn't revoke it with the provider or change your environment.

## What a provider can see

| `privacy` | Sent with each request |
| --- | --- |
| `strict` (default) | Your query and candidate folder names |
| `balanced` | Plus each folder's parent name, the current folder's name and a low/medium/high visit level |
| `full` | Like `balanced`, but with full parent and current folder paths |

File contents, Git remotes, environment variables, shell history and credentials are never sent. The same rules apply
to local Tev1. Folder names and queries can themselves be sensitive:

- `jjump preview "my cv"` prints the exact request without sending it.
- `no_send` folders and their subfolders stay searchable locally but are never sent. While you're inside one, semantic
  requests are off.
- `exclude` folders and their subfolders are never recorded or searched. Direct paths to them still work.

```sh
jjump config set no_send '["/work/private-client"]'
jjump config set exclude '["/work/scratch"]'
```

## History and data

J-Jump records the folder you're in each time your prompt appears after a directory change. It never records commands.

```sh
jjump history list
jjump explain pay                   # how matches for "pay" rank, and where j would go
jjump history forget --preview -- /work/old-project
jjump history prune                 # preview visits to folders that no longer exist
mkdir -m 700 ~/jjump-backup && jjump history backup ~/jjump-backup/visits.json
jjump history restore ~/jjump-backup/visits.json    # validates; add --apply to replace
jjump history clear --preview
jjump cache clear --preview         # cached semantic answers only
jjump data clear --preview          # history and cached answers
jjump config reset --preview        # default settings; keeps history, exclusions and keys
jjump config recover                # preview recovery from a broken config file
```

Anything that deletes or replaces data previews first; add `--apply` to do it. Backups must sit in a private
(`chmod 700`) folder. None of this is secure erasure, and nothing touches your shell's own history.

J-Jump is pre-1.0 and reads only its current file formats. Older or unknown files are left untouched and refused;
`jjump doctor` names the file and the fix.

## Troubleshooting

| Problem | Fix |
| --- | --- |
| `j: command not found` after installing | Open a new terminal. If it persists, run `jjump shell install` and follow what it reports. |
| `j foo` finds nothing | J-Jump only knows folders visited since you installed it. `cd` there once, or check with `jjump explain foo`. |
| `j` or `ji` is already taken | `jjump shell install --cmd jump` gives you `jump` and `jumpi`. |
| <kbd>Tab</kbd> doesn't insert a choice in Bash | Your terminal didn't answer Bash's cursor query. Use `ji foo` instead. |
| Linux can't save a key | Start a Secret Service session (for example GNOME Keyring), or use the provider's environment variable. |
| Tev1 doesn't answer | Make sure Ollama is running and the model is pulled, then run `jjump provider-check`. A cold start may need a retry. |
| An error mentions an old or unknown format | Run `jjump doctor` for the exact remedy. |
| Anything else | Run `jjump doctor`; it's offline and suggests next steps. |

Exit codes: `2` invalid input, `3` no match, `4` selection required, `5` provider, `6` path, `7` state,
`130` cancelled.
