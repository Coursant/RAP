# ![logo](https://raw.githubusercontent.com/Artisan-Lab/RAPx/main/logo.png)
RAPx (Rust Analysis Platform with Extensions) [![license](https://img.shields.io/github/license/Artisan-Lab/RAPx)](./LICENSE-MPL)[![docs.rs](https://img.shields.io/docsrs/rapx)](https://docs.rs/rapx) is an advanced static analysis platform for Rust, developed by researchers at [Artisan-Lab](https://hxuhack.github.io), Fudan University. It provides an extensible framework for building and integrating powerful analysis capabilities that go beyond those available in the standard rustc compiler, empowering developers to reason about safety, robustness, and performance at a deeper level.

RAPx is available on crates.io. [![crates.io](https://img.shields.io/crates/v/rapx.svg)](https://crates.io/crates/rapx)

## Features
# ![logo](https://raw.githubusercontent.com/Artisan-Lab/RAPx/main/feature.png)
RAPx is structured into two layers: a core layer offering essential program analysis algorithms (e.g., alias and dataflow analysis), and an application layer implementing specific tasks such as bug detection. This separation of concerns promotes modular development and fosters collaboration between algorithm and application developers.

The project is still under heavy development. For further details, please refer to the [RAPx-Book](https://artisan-lab.github.io/RAPx-Book).

## Quick Start

Install `nightly-2025-12-06` on which rapx is compiled with. This just needs to do once on your machine. If the toolchain exists,
this will do nothing.

```shell
rustup toolchain install nightly-2025-12-06 --profile minimal --component rustc-dev,rust-src,llvm-tools-preview
cargo +nightly-2025-12-06 install rapx --git https://github.com/Artisan-Lab/RAPx.git
```

## Usage

Navigate to your Rust project folder containing a `Cargo.toml` file. Then run `rapx` by manually specifying the toolchain version according to the [toolchain override shorthand syntax](https://rust-lang.github.io/rustup/overrides.html#toolchain-override-shorthand).

```shell
cargo +nightly-2025-12-06 rapx [rapx options] -- [cargo check options]
```

or by setting up default toolchain to the required version.
```shell
rustup default nightly-2025-12-06
```

Check out supported options with `-help`:

```shell
$ cargo rapx -help

Usage:
    cargo rapx [rapx options or rustc options] -- [cargo check options]

RAPx Options:

Application:
    -F or -uaf      use-after-free/double free detection.
    -M or -mleak    memory leakage detection.
    -O or -opt      automatically detect code optimization chances.
    -I or -infer    (under development) infer the safety properties required by unsafe APIs.
    -V or -verify   (under development) verify if the safety requirements of unsafe API are satisfied.

Analysis:
    -alias          perform alias analysis (meet-over-paths by default)
    -adg            generate API dependency graphs
    -upg            generate unsafety propagation graphs for each module.
    -upg-std        generate unsafety propagation graphs for each module of the Rust standard library
    -callgraph      generate callgraphs
    -dataflow       generate dataflow graphs
    -ownedheap      analyze if the type holds a piece of memory on heap
    -pathcond       extract path constraints
    -range          perform range analysis

General command: 
    -help           show help information
    -version        show the version of RAPx

NOTE: multiple detections can be processed in single run by 
appending the options to the arguments. Like `cargo rapx -F -M`
will perform two kinds of detection in a row.

e.g.
1. detect use-after-free and memory leak for a riscv target:
   cargo rapx -F -M -- --target riscv64gc-unknown-none-elf
2. detect use-after-free and memory leak for tests:
   cargo rapx -F -M -- --tests
3. detect use-after-free and memory leak for all members:
   cargo rapx -F -M -- --workspace

Environment Variables (Values are case insensitive):
    RAP_LOG          verbosity of logging: trace, debug, info, warn
                     trace: print all the detailed RAP execution traces.
                     debug: display intermidiate analysis results.
                     warn: show bugs detected only.

    RAP_CLEAN        run cargo clean before check: true, false
                     * true is the default value except that false is set

    RAP_RECURSIVE    scope of packages to check: none, shallow, deep
                     * none or the variable not set: check for current folder
                     * shallow: check for current workpace members
                     * deep: check for all workspaces from current folder
                      
                     NOTE: for shallow or deep, rapx will enter each member
                     folder to do the check.
```

If RAPx gets stuck after executing `cargo clean`, try manually downloading metadata dependencies by running `cargo metadata`. 

RAPx supports the following environment variables (values are case insensitive):

| var             | default when absent | one of these values | description                  |
|-----------------|---------------------|---------------------|------------------------------|
| `RAP_LOG`       | info                | debug, info, warn   | verbosity of logging         |
| `RAP_CLEAN`     | true                | true, false         | run cargo clean before check |
| `RAP_RECURSIVE` | none                | none, shallow, deep | scope of packages to check   |

For `RAP_RECURSIVE`:
* none: check for current folder
* shallow: check for current workpace members
* deep: check for all workspaces from current folder
 
NOTE: rapx will enter each member folder to do the check.


### Collect BC JSON dataset from popular crates

This repository provides a helper script to build a dataset from popular open-source crates:

```shell
python3 collect_popular_crates_bc_dataset.py \
  --top-n 10 \
  --output-dir dataset_bc \
  --toolchain nightly-2025-12-06
```

What it does:
1. Fetches top crates from crates.io (by downloads) with pagination and saves a reproducible snapshot file such as `popular_crates_top1000.json`
2. Downloads each crate source tarball into `dataset_bc/sources/`
3. Runs `cargo rapx -bounds-db` per crate with adaptive toolchain selection:
   - prefer crate-local `rust-toolchain.toml` / `rust-toolchain`
   - fallback to `Cargo.toml` `rust-version`
   - fallback to `--toolchain`, then default cargo toolchain
   - workspace crates are analyzed with `RAP_RECURSIVE=shallow`
4. Finds generated bounds-check JSON (`bounds_checks*.json`)
5. Builds `bounds_checks_dataset.json` where each record contains:
   - one BC entry (`bc`)
   - crate metadata (`crate`, `version`, `rank`)
   - the corresponding LLVM reserved marker (`llvm_reserved`)
   - match status (`llvm_reserved_matched`, unmatched rows keep `llvm_reserved = null`)
   - LLVM retention result (`llvm_retained`)

Output files:
- `dataset_bc/popular_crates_top*.json`: frozen popular-crate snapshot used by the run
- `dataset_bc/raw_json/*.json`: copied raw BC JSON per crate
- `dataset_bc/crate_status/*.json`: per-crate processing status and counts
- `dataset_bc/bounds_checks_dataset.json`: aggregated dataset with `metadata`, `crates`, and `records`
- `dataset_bc/dataset_index.json`: top-level summary and links to per-crate status files
  - if crates.io is unreachable, `dataset_index.json` is still written with `status = "fetch_popular_crates_failed"` and the error message

Offline mode:

```shell
python3 collect_popular_crates_bc_dataset.py \
  --offline \
  --output-dir dataset_bc
```

- `--offline` keeps the same analysis pipeline, but switches crate source acquisition from crates.io downloads to existing sources under `<output-dir>/sources`.
- `--offline` does not require `--crates-file`; it auto-discovers crates from `<output-dir>/sources`.
- If needed, `--crates-file` can still be provided to restrict offline analysis to a subset of crates already present in `sources/`.
- Supported layouts inside `<output-dir>/sources` include:
  - `<output-dir>/sources/<crate>-<version>`
  - `<output-dir>/sources/<crate>/<version>`
  - `<output-dir>/sources/<crate>`
