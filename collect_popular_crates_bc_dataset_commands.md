# `collect_popular_crates_bc_dataset.py` Common Commands

## Arguments You Will Commonly Reset

These arguments are the ones you will usually change between runs:

- `--top-n`
  Online mode only. Change this when switching between quick sampling and full runs.
- `--crates-file`
  Change this when using a different fixed crate list.
- `--output-dir`
  Change this for different experiments so outputs do not overwrite each other.
- `--toolchain`
  Change this when comparing results across toolchains or when a crate set needs a different Rust version.
- `--timeout-sec`
  Change this depending on whether you want quicker failure or maximum coverage.

## Arguments Usually Kept As-Is

- `--offline`
  This is a mode switch, not a tuning parameter. Only set it when you want local sources instead of network downloads.
- If you are doing the standard full online experiment, the stable baseline command is:
  - `--top-n 1000`
  - `--output-dir dataset_bc`
  - `--toolchain nightly-2025-12-06`
  - `--timeout-sec 1800`

## Online Mode

Fetch popular crates from crates.io, download sources, run `rapx -bounds-db`, and build the dataset:

```bash
python collect_popular_crates_bc_dataset.py \
  --top-n 1000 \
  --output-dir dataset_bc \
  --toolchain nightly-2025-12-06 \
  --timeout-sec 1800
```

Commonly reset here:
- `--top-n`
- `--output-dir`
- `--toolchain`
- `--timeout-sec`

Use a smaller sample for quick validation:

```bash
python collect_popular_crates_bc_dataset.py \
  --top-n 10 \
  --output-dir dataset_bc_test \
  --toolchain nightly-2025-12-06 \
  --timeout-sec 600
```

## Fixed Crate List Mode

Use a local crate list file instead of fetching the popular-crate snapshot:

```bash
python collect_popular_crates_bc_dataset.py \
  --crates-file crates.txt \
  --output-dir dataset_bc \
  --toolchain nightly-2025-12-06 \
  --timeout-sec 1800
```

Commonly reset here:
- `--crates-file`
- `--output-dir`
- `--toolchain`
- `--timeout-sec`

Example `crates.txt`:

```text
syn@2.0.117
quote@1.0.39
serde 1.0.219
tokio@1.44.2
```

Example `crates.json`:

```json
[
  { "name": "syn", "version": "2.0.117" },
  { "name": "quote", "version": "1.0.39" },
  { "name": "serde", "version": "1.0.219" }
]
```

Run with JSON input:

```bash
python collect_popular_crates_bc_dataset.py \
  --crates-file crates.json \
  --output-dir dataset_bc \
  --toolchain nightly-2025-12-06
```

## Offline Mode

Keep the same analysis pipeline, but read crate sources from a local directory instead of downloading from crates.io:

```bash
python collect_popular_crates_bc_dataset.py \
  --offline \
  --output-dir dataset_bc \
  --toolchain nightly-2025-12-06 \
  --timeout-sec 1800
```

`--offline` does not require `--crates-file`. By default it auto-discovers crates from `<output-dir>/sources`.

Optional:

- `--crates-file`
  Use this only if you want to restrict offline analysis to a subset of crates already present in `sources/`.

Commonly reset here:
- `--output-dir`
- `--toolchain`
- `--timeout-sec`

Before running offline mode, prepare the source tree under `<output-dir>/sources`.

Supported layouts inside `sources/`:

- `<output-dir>/sources/<crate>-<version>`
- `<output-dir>/sources/<crate>/<version>`
- `<output-dir>/sources/<crate>`

Example:

```text
dataset_bc/
└── sources/
    ├── syn-2.0.117/
    └── quote-1.0.39/
```

Optional offline subset run:

```bash
python collect_popular_crates_bc_dataset.py \
  --offline \
  --crates-file crates.txt \
  --output-dir dataset_bc
```

## Useful Variants

Write results to a custom directory:

```bash
python collect_popular_crates_bc_dataset.py \
  --crates-file crates.txt \
  --output-dir /tmp/rap_bc_dataset
```

Use a different toolchain:

```bash
python collect_popular_crates_bc_dataset.py \
  --crates-file crates.txt \
  --toolchain stable
```

Reduce timeout for quick failure:

```bash
python collect_popular_crates_bc_dataset.py \
  --crates-file crates.txt \
  --timeout-sec 300
```

## Function Testcase Extraction

After `bounds_checks_dataset.json` has been generated, extract standalone
function-level testcase crates for the functions that contain bounds checks:

```bash
python3 extract_bc_function_testcases.py \
  --dataset-path dataset_bc/bounds_checks_dataset.json \
  --sources-dir dataset_bc/sources \
  --output-dir dataset_bc/function_testcases \
  --timeout-sec 120
```

This command groups records by function, writes one minimal Cargo crate per
function, and runs `cargo check` for each generated testcase. Only generated
crates whose `cargo check` succeeds are marked with `check_status = "ok"`.
Failures are still kept in the index with diagnostic statuses such as:

- `source_not_found`
- `function_not_found`
- `unsupported_nested_context`
- `cargo_check_failed`
- `timeout`

Only extract bounds checks that LLVM retained:

```bash
python3 extract_bc_function_testcases.py \
  --dataset-path dataset_bc/bounds_checks_dataset.json \
  --sources-dir dataset_bc/sources \
  --output-dir dataset_bc/function_testcases_retained \
  --only-retained
```

## Test Command

Run the unit tests for the dataset collector and testcase extractor:

```bash
python3 -m unittest test_collect_popular_crates_bc_dataset.py test_extract_bc_function_testcases.py
python3 -m py_compile collect_popular_crates_bc_dataset.py extract_bc_function_testcases.py test_extract_bc_function_testcases.py
```

## Output Files

Main outputs are written under the chosen `--output-dir`:

- `popular_crates_top*.json`
- `sources/`
- `logs/`
- `run_reports/run_report_<timestamp>.log`
- `raw_json/`
- `crate_status/`
- `bounds_checks_dataset.json`
- `dataset_index.json`

`run_reports/run_report_<timestamp>.log` is the detailed run-level report for one execution. Reports are not overwritten between runs.

Function testcase extraction writes its own outputs under the extractor
`--output-dir`:

- `function_testcases_index.json`
- `run_reports/extract_report_<timestamp>.log`
- `<crate>-<version>/<function-slug>/Cargo.toml`
- `<crate>-<version>/<function-slug>/src/lib.rs`
- `<crate>-<version>/<function-slug>/bc_metadata.json`

`run_reports/extract_report_<timestamp>.log` summarizes one extraction run,
including configuration, aggregate counts, status counts, per-testcase rows, and
failure details. Extraction reports are not overwritten between runs.
