# pngmetagrep

PNG tEXt metadata extractor — NDJSON output for `jq` pipelines.

Extracts `tEXt` chunks from PNG files and outputs one JSON object per line (NDJSON). No image decoding — reads only binary chunk headers for speed.

## Use Cases

- **VDSL** — search/aggregate `vdsl` recipe chunks embedded by the VDSL image generation platform
- **ComfyUI** — extract `prompt` / `workflow` chunks
- **General** — any arbitrary tEXt keyword

## Install

```bash
cargo install pngmetagrep
```

## Usage

```bash
# Extract vdsl chunks (default) from all PNGs under a directory
pngmetagrep ./images

# Specify chunk keywords (repeatable)
pngmetagrep ./images --chunk prompt --chunk workflow

# Regex filter on JSON output
pngmetagrep ./images -e '"seed":\s*42'

# Case-insensitive filter
pngmetagrep ./images -e 'landscape' -i

# Print matching file paths only (no JSON)
pngmetagrep ./images -e 'portrait' -l

# Limit parallel threads
pngmetagrep ./images -j 4

# Pipe to jq
pngmetagrep ./images | jq 'select(.seed == 42)'
```

## Options

| Flag | Description |
|---|---|
| `--chunk <KEY>` | tEXt keyword to extract (repeatable, default: `vdsl`) |
| `-e <REGEX>` | Regex filter applied to serialized JSON output |
| `-i` | Case-insensitive matching for `-e` |
| `-l` | Print matching file paths only (no JSON) |
| `-j <N>` | Number of parallel threads (default: CPU count) |

## Output Format

Single chunk whose value is a JSON object — `path` is merged flat:

```json
{"path":"images/001.png","_v":1,"seed":42,"model":"sd-xl"}
```

Multiple chunks or non-object values — nested by keyword:

```json
{"path":"images/002.png","prompt":{...},"workflow":{...}}
```

## Crate Structure

| Crate | Role |
|---|---|
| `pngmetagrep-core` | PNG tEXt chunk extraction library (std only, no image decoding) |
| `pngmetagrep` (CLI) | Parallel CLI built on clap + rayon + walkdir |

## License

MIT
