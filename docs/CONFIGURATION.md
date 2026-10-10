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
| `provider` | `jev`, `clef-flash`, `tev1`, `openai`, `dsl` | `jev` |
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

## Declarative APIs (0.0.43+)

A versioned JSON definition can configure an API without adding a Rust provider. The four built-in providers use the
same JSON wire interpreter; their existing names, settings and credential entries remain available. An imported
definition occupies the `dsl` slot. Import another definition to replace that slot; selecting a built-in retains it.
Definitions are trusted configuration: review their endpoint and transformations before importing them.

```sh
jjump config provider-example openrouter-jev > provider.json
# Edit provider.json if needed; it contains configuration, never the key.
jjump config import-provider provider.json
export OPENROUTER_API_KEY="your-openrouter-key" # or: jjump credential set
jjump config set semantic on
jjump preview backend                       # inspect the exact filtered request offline
```

`provider-example` also accepts `openrouter-microsoft`, `openrouter-openai`, `openrouter-clef`, `vercel-openai` and
`local-tev1`. The three additional OpenRouter examples use its alpha Decisions route with the same System One-shaped
choice contract. Jev's template explicitly admits the dated model ID documented by OpenRouter; other templates accept
the requested model only unless you add a verified response alias. The gateway examples target OpenRouter's
[System One endpoint](https://openrouter.ai/docs/guides/community/typesafe-sdk) and Vercel's
[OpenAI-compatible Decisions endpoint](https://vercel.com/i/ai-sdk-evaluation-to-decisions). They are synthetic format
tested; live account permissions and model quality are not tested. Gateway API changes may require editing the definition.
The editable files are in [examples/providers](../examples/providers/). A model served in a different protocol needs the
matching wire definition, not just a changed URL. Model fallbacks with unlisted response names are rejected.

Import validates offline, selects `dsl`, and copies the definition into private configuration. The source file can then
be removed. Semantic enablement, consent and privacy settings are preserved. Import resets the custom credential source
to `environment`; it never writes or deletes an OS key. `credential set` stores under `j-jump.dsl.DEFINITION-ID`, account
`api-key`, separate from every built-in and other definition ID. Profiles with the same definition ID share that OS entry.
The explicitly named, nonempty environment variable takes priority. Secret values are resolved only in transport and are
never available to expressions, preview, definition hashes or diagnostics.

### Definition format

The complete examples are runnable templates. Top-level fields are strict:

| Field | Meaning |
| --- | --- |
| `version` | `1`; unknown versions and fields fail before saving |
| `id` | 1–64 lowercase letters, digits or hyphens; credential namespace |
| `model` | Exact request model, also required at the root of the wire request |
| `url` | Complete JSON POST endpoint; no credentials, query or fragment |
| `transport` | `https`, or `loopback` for numeric `127.0.0.1` / `[::1]` HTTP |
| `auth` | `{"type":"none"}`, `{"type":"bearer","env":"MY_API_KEY"}`, or `{"type":"header","env":"MY_API_KEY","name":"x-api-key"}` |
| `limits` | Optional `candidates` (1–254), `body_bytes` (256–65536), `context_bytes` (optional 256–65536), `diverse`, `cache` |
| `response_models` | Optional explicit response model aliases; no wildcards; max 16 |
| `wire` | `request`, `response`, `questions`, optional `request_shape`, `response_shape`, `response_checks` |
| `probe` | Optional finite local check plan, described below |

Default limits: 254 candidates, 64 KiB body, no context limit, no diversity adjustment, cache off. Local responses are
never persisted in the answer cache even if `cache` is requested. Cloud endpoints honor configured proxy settings;
loopback always connects directly. Redirects are disabled. Loopback definitions must use `auth: {"type":"none"}`.
Host, content framing and proxy credential headers cannot be overridden. Only JSON POST APIs and bounded local GET/POST
probes are supported; streaming, OAuth exchange, arbitrary signing, service installation and downloads need host support.

### Expressions and source validation

An expression is a tagged JSON object. Fields not listed for its operation are errors.

| `op` | Fields and result |
| --- | --- |
| `literal` | `value`: return literal JSON |
| `ref` | `path`: JSON Pointer into the current root; optional `default` (including null) for missing values |
| `object` | `fields`: object mapping keys to expressions |
| `array` | `items`: array of expressions |
| `entries` | `input`: turn an object into `[{"key":...,"value":...}]` |
| `map` | `input`, `body`: evaluate body for each array item |
| `map_values` | `input`, `body`: transform object values, retaining keys |
| `filter` | `input`, `condition`: retain array items whose condition is true |
| `index` | `input`, `key`, `value`: array to object; duplicate keys are errors |
| `stringify` | `input`: serialize any JSON as a string |
| `text` | `input`: preserve strings; serialize other JSON as a string |
| `if` | `condition`, `then`, `otherwise`: evaluate the selected branch |
| `eq`, `le`, `add` | `left`, `right`: JSON equality, unsigned integer comparison or checked addition |
| `length` | `input`: object/array count or string UTF-8 byte length |
| `all`, `any` | `items`: boolean conjunction/disjunction; every operand is checked |
| `not` | `input`: boolean negation |
| `version_at_least` | `input`, `minimum`: compare a three-part version with `[major,minor,patch]`; suffixes are ignored |

Request roots contain only `task` (already filtered `state` and named `questions`) and `model`. Response roots contain
only `response`; question recovery roots contain only `request`. Mapping operations bind the current element as `item`,
replacing that binding inside nested maps. There is no environment, file, shell, script, URL fetching or secret operation.
Request recovery must preserve the exact question and choice IDs, including `none`, before a request can be sent.
Response normalization must produce the shared `{model, answers, usage?}` format. Core checks still enforce model and
question identity, choice IDs, refusal handling, finite probabilities, distribution sums, maximum choice and abstention.

A shape supports `kind` (`object`, `array`, `string`, `uint`, `number`, `boolean`, `null`, or empty for unconstrained JSON),
`properties`, `required`, `extra`, `items`, `values`, `choices`, `any`, `min`, and `max`. Objects reject unknown fields by
default; `values` validates dynamic object entries. `any` accepts one matching shape. `min`/`max` bound collection sizes
or string byte lengths. `response_checks` is an array of boolean expressions, all required to be true before normalization.
Gateway definitions explicitly admit documented metadata and project it out; malformed decision fields remain errors.

Definitions are at most 32 KiB; expressions have at most 4096 nodes and depth 32. Values are bounded to 1024 entries per
collection, depth 32 and 256 KiB serialized size. Each expression evaluation has 100,000 work units; response assertions and normalization share one budget; intermediate output
bytes and copied mapping scopes consume additional units. The core crops the candidate prefix to the configured body/context budget before sending.
Unsupported operations, missing references, duplicate JSON keys, duplicate indices, wrong types and exhausted limits fail
closed. The full definition digest participates in endpoint binding, cache and circuit identities. Adapter configuration
fingerprints prevent a pending request from crossing a concurrent definition change.

### Local Tev and probes

The built-in `tev1` setup and model discovery continue to work. To use the entirely configurable local example:

```sh
jjump config provider-example local-tev1 > local-provider.json
# Edit its loopback URL/model if needed, after installing Ollama and compatible GGUF weights.
jjump config import-provider local-provider.json
jjump provider-check --json
```

The example checks Ollama version, confirms the selected GGUF tag, then sends a synthetic ready/none decision. It retains
Tev's 23-candidate and 1600-byte context limits, with no answer cache. It never starts Ollama or downloads weights.

A `probe` has `steps` (1–7) and a `report` expression. Each step has an absolute same-origin `path`, optional synthetic
`task`, optional `expect_choice`, and optional boolean `checks`. A step without a task uses GET; one with a task adapts it
through `wire.request` and uses POST. Checks run after each reply with `response`, `responses`, and `model`; an expected
choice must pass core decision validation. The report must evaluate to an object and sees `responses` and `model`.
Only explicit `provider-check` runs the plan, within the shared 10-second deadline; import/setup/doctor/preview do not.
Custom model discovery is currently unsupported; built-in Tev retains its installed-model selector.
