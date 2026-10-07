# rmedia documentation

rmedia cuts clips from videos on Linux and Windows, from the `rmedia` command line or the desktop
app. Start with the page for your role.

## Users

| Page | What it covers |
|---|---|
| [Getting started](user/getting-started.md) | Install, check the engine, cut a first clip |
| [Desktop guide](user/desktop-guide.md) | Sources, player, marking, queue, run, export |
| [CLI guide](user/cli-guide.md) | `clip`, clip lists, folders, `batch`, `scan`, `doctor` by task |
| [Troubleshooting](user/troubleshooting.md) | Common errors and what to do |

## Administrators

| Page | What it covers |
|---|---|
| [Installation](administration/installation.md) | Packages per platform, bundled FFmpeg, uninstall |
| [Configuration](administration/configuration.md) | Settings file, environment variables, folders |

## Reference

| Page | What it covers |
|---|---|
| [CLI reference](api/cli-reference.md) | Every command, option, default and exit code |
| [File formats](api/file-formats.md) | Times, clip lists, path lists, JSON/CSV manifests, name templates |
| [JSON output](api/json-output.md) | `--json` documents and NDJSON run events |
| [Desktop IPC](api/desktop-ipc.md) | Tauri commands, events, path policy, media server |
| [Glossary](reference/glossary.md) | Terms used across the project |

## Developers and maintainers

| Page | What it covers |
|---|---|
| [Architecture overview](architecture/overview.md) | Core and surfaces, process model, key decisions |
| [Domains](architecture/domains.md) | Each module: responsibilities, types, invariants |
| [Security model](security/security-model.md) | Threats, controls, residual risks |
| [Development guide](developer/development-guide.md) | Setup, build, workflow, conventions |
| [Testing](developer/testing.md) | Test suites, fixtures, benchmarks |
| [Build and release](operations/build-and-release.md) | CI, packaging, pinned FFmpeg, releasing |
| [Release notes 0.1.0](releases/0.1.0.md) | What feature 001 delivers; known issues |

Specifications, plans and contracts live in [`specs/`](../specs/); research notes are in
[`research/`](../research/).
