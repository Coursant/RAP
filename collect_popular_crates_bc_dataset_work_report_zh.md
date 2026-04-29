# `collect_popular_crates_bc_dataset` 对话工作报告

## 1. 背景与目标

本次对话围绕 `collect_popular_crates_bc_dataset.py` 和 `rapx -bounds-db` 展开，目标是把现有的原型脚本扩展成一套可用于实验的批量化工具链，用于扫描 Rust 热门库中的 bounds check，并收集这些 bounds check 在 LLVM 中是否被保留/优化消除的情况。

核心要求包括：

- 基于现有 `rapx -bounds-db` 输出，不重构单条 bounds check 的核心字段。
- 支持批量实验，面向前 1000 个热门 crate 或给定 crate 列表。
- 结果以 JSON 形式持久化，不再以 JSONL 作为主输出。
- 支持在线模式与离线模式。
- 为每次运行提供更清晰、更适合实验排障的总体报告。

---

## 2. 主要工作内容

### 2.1 重构数据收集脚本

对 [collect_popular_crates_bc_dataset.py](/home/lcc/rust/RAP/collect_popular_crates_bc_dataset.py) 进行了大幅重构，主要改动如下：

- 将热门 crate 拉取从“只抓第一页”改为支持分页抓取。
- 将默认 `top_n` 调整为 `1000`。
- 引入热门 crate 快照文件：
  - `popular_crates_top*.json`
- 输出从原来的 JSONL 主文件改为分层 JSON：
  - `bounds_checks_dataset.json`
  - `dataset_index.json`
  - `crate_status/*.json`
  - `raw_json/*.json`
- 修复并增强了 bounds check 与 LLVM 记录的对齐逻辑：
  - 正确读取 `llvm.reserved.records`
  - 支持按嵌套的 `location.file` / `location.line` 匹配
  - 生成 `llvm_reserved_matched` 与 `llvm_retained`
- 对 workspace crate 默认设置 `RAP_RECURSIVE=shallow`
- 增加 toolchain fallback 过程记录
- 增加每个 crate 的独立状态 JSON 与日志文件

### 2.2 离线模式设计与演进

离线模式经历了两轮设计收敛：

第一阶段：
- 增加 `--offline`
- 一度通过额外的本地源码目录输入进行离线分析

第二阶段：
- 进一步简化离线语义
- 当前离线模式不再依赖 `--local-sources-dir`
- 当前离线模式默认直接扫描 `--output-dir/sources` 下已有源码

当前离线模式支持两种子模式：

1. 自动发现模式
- `source_mode = offline_existing_sources`
- 不要求 `--crates-file`
- 自动扫描 `sources/` 中可识别的 crate

2. 子集过滤模式
- `source_mode = offline_fixed_crates`
- 可选提供 `--crates-file`
- 只分析 `sources/` 中已存在且被列表选中的 crate

### 2.3 运行总体报告能力

新增了运行级别的文本报告：

- `run_reports/run_report_<timestamp>.log`

每次运行都会生成独立的带时间戳报告，不覆盖历史报告。报告不是单 crate 编译日志，而是整次运行的总览，包含：

- 开始时间、结束时间、总耗时
- 运行配置
- 总体指标
- 按状态统计
- 每 crate 汇总表格
- 失败详情区块

### 2.4 运行报告详细增强

报告功能在后续几轮对话中持续增强，最终形态包括：

#### 总体统计增强

- `requested_crates`
- `success_count`
- `failed_count`
- `total_bc_count`
- `matched_bc_count`
- `retained_bc_count`
- `matched_ratio`
- `retained_ratio`

#### `Per-Crate Summary` 表格化

将逐 crate 摘要改成固定列宽文本表格，当前列包括：

- `crate`
- `version`
- `rank`
- `status`
- `toolchain`
- `elapsed`
- `bc`
- `matched`
- `retained`
- `origin`
- `status_file`

#### `Failures Detail` 区块

为失败 crate 单独提供详细信息：

- `crate@version`
- `status`
- `toolchain`
- `last_cmd`
- `error`
- `status_file`

若失败时存在日志尾部摘要，还会额外输出：

- `log_tail`

### 2.5 失败诊断增强

为了让运行报告对失败案例真正有用，又增加了以下诊断字段：

- `last_attempt_command`
- `log_tail`

这两个字段同时写入：

- `crate_status/<crate>-<version>.json`
- `dataset_index.json` 的 `crates[*]`
- 运行报告的 `Failures Detail`

这意味着在大批量实验失败时，不需要先打开完整的 `logs/*.log`，先看运行总报告就可以拿到最后一次命令和输出尾部。

### 2.6 文档补充

同步完善了文档：

- 更新 [README.md](/home/lcc/rust/RAP/README.md)
- 新增 [collect_popular_crates_bc_dataset_commands.md](/home/lcc/rust/RAP/collect_popular_crates_bc_dataset_commands.md)

命令文档中补充了：

- 在线模式用法
- 固定列表模式用法
- 离线模式用法
- 常用参数说明
- 哪些参数通常需要按实验重设
- 输出文件说明
- 运行报告路径的定位

### 2.7 函数级 testcase 提取器

新增独立脚本 [extract_bc_function_testcases.py](/home/lcc/rust/RAP/extract_bc_function_testcases.py)，用于在已有数据集基础上，把包含 bounds check 的函数提取成可单独验证的 testcase crate。

该脚本的设计目标是：

- 不改变 `collect_popular_crates_bc_dataset.py` 主采集流程
- 读取已有 `bounds_checks_dataset.json`
- 读取已有 `<output-dir>/sources`
- 按 `bc.function_context.name` 将同一函数内多个 bounds check 合并
- 每个函数生成一个最小 Cargo crate
- 对生成的 testcase 执行 `cargo check`
- 只有 `cargo check` 成功的 testcase 才标记为 `check_status = ok`

当前提取策略为标准库实现的轻量 Rust item 扫描：

- 通过 `bc.location.file` / `bc.location.line` 定位源码
- 在目标行附近寻找包含该行的 `fn`
- 包含紧邻的 doc comments 与 attributes
- 使用 brace matching 找到函数结束位置
- 第一版只支持顶层自由函数
- 对 `impl` / `trait` / `mod` 内部函数记录 `unsupported_nested_context`

新增输出包括：

- `function_testcases_index.json`
- `run_reports/extract_report_<timestamp>.log`
- 每个 testcase crate 的 `Cargo.toml`
- 每个 testcase crate 的 `src/lib.rs`
- 每个 testcase crate 的 `bc_metadata.json`

提取器也会生成独立运行报告，包含配置、总体计数、状态分布、逐 testcase 摘要和失败详情；报告按时间戳保存，不覆盖历史报告。

失败项不会伪装成可用 testcase，而是记录明确状态：

- `source_not_found`
- `function_not_found`
- `unsupported_nested_context`
- `cargo_check_failed`
- `timeout`

---

## 3. 当前脚本最终行为

### 3.1 在线模式

适用于直接从 crates.io 获取实验对象：

- 可按下载量分页抓取热门 crate
- 可固化快照
- 下载源码到 `sources/`
- 运行 `rapx -bounds-db`
- 落盘 JSON 数据集和运行报告

### 3.2 固定列表模式

适用于用户自己指定 crate 集合：

- 使用 `--crates-file`
- 在线下载源码
- 后续分析流程与在线模式一致

### 3.3 离线模式

适用于已经准备好本地源码的情况：

- 使用 `--offline`
- 默认扫描 `--output-dir/sources`
- 自动识别 crate 名称与版本
- 不再强制要求 `--crates-file`
- 如需只跑子集，可额外提供 `--crates-file`

### 3.4 输出结构

当前主输出目录结构为：

```text
<output-dir>/
├── popular_crates_top*.json
├── bounds_checks_dataset.json
├── dataset_index.json
├── run_reports/
│   └── run_report_<timestamp>.log
├── sources/
├── logs/
├── raw_json/
└── crate_status/
```

其中：

- `bounds_checks_dataset.json`
  聚合后的主数据集
- `dataset_index.json`
  运行索引与总体摘要
- `run_reports/run_report_<timestamp>.log`
  人工审阅用的单次运行报告，历史报告不会被覆盖
- `crate_status/*.json`
  每个 crate 的详细状态与诊断信息

### 3.5 函数 testcase 提取流程

在主数据集生成后，可以运行：

```bash
python3 extract_bc_function_testcases.py \
  --dataset-path dataset_bc/bounds_checks_dataset.json \
  --sources-dir dataset_bc/sources \
  --output-dir dataset_bc/function_testcases \
  --timeout-sec 120
```

如只关注 LLVM retained 的 bounds check，可额外使用：

```bash
python3 extract_bc_function_testcases.py \
  --only-retained
```

提取器会生成：

```text
<function-testcases-output>/
├── function_testcases_index.json
├── run_reports/
│   └── extract_report_<timestamp>.log
└── <crate>-<version>/
    └── <function-slug>/
        ├── Cargo.toml
        ├── bc_metadata.json
        └── src/
            └── lib.rs
```

---

## 4. 测试与验证工作

测试文件 [test_collect_popular_crates_bc_dataset.py](/home/lcc/rust/RAP/test_collect_popular_crates_bc_dataset.py) 已同步扩展，当前覆盖的重点包括：

- 热门 crate 拉取支持分页
- 网络失败时仍会落盘 `dataset_index.json` 与独立运行报告
- `run_rapx()` 的 toolchain fallback 逻辑
- workspace crate 默认使用 `RAP_RECURSIVE=shallow`
- 能正确读取 `llvm.reserved.records`
- 能按 nested `location` 匹配 LLVM 记录
- 能正确生成 `bounds_checks_dataset.json`
- 支持固定列表模式
- 支持离线自动发现模式
- 支持离线过滤子集模式
- 支持离线 `sources/` 缺失时的失败路径
- 支持运行报告的表格与失败详情断言
- 支持失败诊断字段 `last_attempt_command` / `log_tail`

新增测试文件 [test_extract_bc_function_testcases.py](/home/lcc/rust/RAP/test_extract_bc_function_testcases.py)，覆盖重点包括：

- 按函数合并多个 bounds check record
- 提取普通顶层函数，并保留 doc comments 与 attributes
- 拒绝 `impl` 内方法并标记 `unsupported_nested_context`
- 生成 testcase crate、`bc_metadata.json` 与 `function_testcases_index.json`
- mock `cargo check` 成功、失败和超时路径
- 记录 `source_not_found` 与 `function_not_found`
- `--only-retained` 过滤 retained bounds check
- 生成独立 extractor run report，并验证重复运行不会覆盖旧报告

本次对话结束时，已执行验证命令：

```bash
python3 -m unittest test_collect_popular_crates_bc_dataset.py test_extract_bc_function_testcases.py
python3 -m py_compile collect_popular_crates_bc_dataset.py extract_bc_function_testcases.py test_extract_bc_function_testcases.py
```

结果：

- `23` 个测试全部通过
- Python 语法检查通过

---

## 5. 本次对话中形成的关键设计结论

### 5.1 离线模式的真实需求

最初“离线模式”的理解是“换一个本地源码目录输入”，但对实际实验场景而言，这会额外引入第二个源码根目录，管理成本更高。

最终更合理的结论是：

- 离线模式应该直接分析 `sources/` 中已有源码
- 不再引入额外输入根目录
- 这更适合已有实验缓存、断点续跑和手工准备源码的场景

### 5.2 运行报告不应只依赖 JSON

虽然已有 `dataset_index.json` 和 `crate_status/*.json`，但在批量实验中，人工首先需要的是“快速审阅”和“快速定位失败原因”。

因此引入了：

- 文本型运行报告

而且后续把它增强为：

- 表格化的逐 crate 概览
- 单独的失败详情区块
- 命令与日志尾部摘要

这个方向是正确的，因为它直接降低了实验排障成本。

### 5.3 保持 `rapx -bounds-db` 字段稳定

整个过程都遵循了一个约束：

- 不重构 `rapx -bounds-db` 单条 bounds check 的核心字段

所有新增信息都放在聚合层、状态层或报告层，不去破坏原始数据形状。这个约束使得后续若要与已有数据或已有分析脚本兼容，成本更低。

### 5.4 testcase 可用性必须经过验证

函数源码切片本身并不等于可单独复现的 testcase。很多函数依赖原 crate 的私有类型、模块路径、宏或 impl 上下文。

因此新增提取器采用的判断标准是：

- 先生成最小 Cargo crate
- 再运行 `cargo check`
- 只有检查通过才认为是可用 testcase
- 失败时保留状态与输出尾部，供后续人工或自动化修复

这避免了把“已提取文本”误判为“可独立测试”的问题。

---

## 6. 当前遗留与可继续改进项

虽然本次对话中的目标已经基本完成，但仍有一些后续可做项：

### 6.1 为 `rapx_failed` 区分更细状态

目前很多失败仍然只归类为：

- `rapx_failed`

后续可以进一步细分为：

- 编译失败
- 超时失败
- JSON 未生成
- LLVM IR 生成失败

### 6.2 结构化运行报告

当前已有文本报告：

- `run_reports/run_report_<timestamp>.log`

后续可以再增加：

- `run_report.json`

这对多次实验结果汇总和自动化比对更合适。

### 6.3 断点续跑 / 跳过已完成 crate

对于真正的前 1000 crate 规模，建议后续增加：

- 断点续跑
- 跳过已完成 crate
- 失败重试策略

### 6.4 数据集分片

当前主数据集是单个：

- `bounds_checks_dataset.json`

当数据规模继续扩大时，可以考虑：

- 分片输出
- 同时保留统一索引文件

### 6.5 提取器支持更多 Rust 上下文

当前 `extract_bc_function_testcases.py` 第一版只支持顶层自由函数。后续可以继续支持：

- `impl` 方法提取
- trait 默认方法提取
- 必要的 enclosing module / impl skeleton 生成
- 原 crate 内部 helper item 的依赖闭包收集
- 对生成 testcase 再运行 `rapx -bounds-db` 验证 bounds check 是否仍可复现

---

## 7. 本次对话最终产物

本次对话直接形成或修改的关键文件包括：

- [collect_popular_crates_bc_dataset.py](/home/lcc/rust/RAP/collect_popular_crates_bc_dataset.py)
- [test_collect_popular_crates_bc_dataset.py](/home/lcc/rust/RAP/test_collect_popular_crates_bc_dataset.py)
- [extract_bc_function_testcases.py](/home/lcc/rust/RAP/extract_bc_function_testcases.py)
- [test_extract_bc_function_testcases.py](/home/lcc/rust/RAP/test_extract_bc_function_testcases.py)
- [README.md](/home/lcc/rust/RAP/README.md)
- [collect_popular_crates_bc_dataset_commands.md](/home/lcc/rust/RAP/collect_popular_crates_bc_dataset_commands.md)

新增的本报告文件：

- [collect_popular_crates_bc_dataset_work_report_zh.md](/home/lcc/rust/RAP/collect_popular_crates_bc_dataset_work_report_zh.md)

---

## 8. 结论

本次对话已经将原先偏原型性质的 `collect_popular_crates_bc_dataset.py`，推进成一套更接近实验工具的实现：

- 支持在线抓取与分页
- 支持固定列表
- 支持离线扫描已有 `sources/`
- 支持分层 JSON 持久化
- 支持更完整的运行级报告
- 支持失败排障摘要
- 支持从 bounds check 数据集中提取函数级 testcase crate
- 支持用 `cargo check` 验证 testcase 是否可独立编译
- 具备对应的单元测试覆盖

从当前状态看，这套工具已经可以用于：

- 小规模功能验证
- 指定 crate 集合分析
- 基于本地源码缓存的离线批量实验
- 对 bounds check 与 LLVM BCE 情况做结构化数据收集
- 从数据集中筛选并落盘可独立检查的函数级 testcase

如果后续要继续投入，优先级最高的增强项应是：

1. 断点续跑
2. 更细失败分类
3. 结构化运行报告 JSON
4. 大规模数据分片输出
5. 提取器支持 `impl` / `trait` / module 上下文
