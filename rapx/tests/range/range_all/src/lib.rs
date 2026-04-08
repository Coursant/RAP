//! 统一的 range 测试集合（单文件版本）。
//!
//! 目标：
//! 1. 覆盖 `rapx/tests/range` 目录已有 testcase 的核心测试要求；
//! 2. 将测试点统一组织在一个 `lib.rs`，便于一次性查看与维护；
//! 3. 每个 testcase 使用注释明确“在测试什么”。

use std::slice;

// ============================================================================
// 一、纯数值计算 / 区间传播
// ============================================================================

/// testcase: 双变量收敛循环（对应 range_1 的核心思想）
///
/// 测试功能：
/// - 验证循环中多个变量（`i`、`j`、`k`）的联合区间更新；
/// - 验证 while 条件与循环体赋值对区间上下界的持续收紧；
/// - 验证在嵌套循环场景下，分析是否能稳定收敛。
pub fn numeric_coupled_loop() {
    let mut k = 0usize;
    while k < 100 {
        let mut i = 0usize;
        let mut j = k;
        while i < j {
            i += 1;
            j -= 1;
        }
        k += 1;
    }
}

/// testcase: 跨函数参数传递与返回值区间（对应 range_2）
///
/// 测试功能：
/// - 验证调用点实参区间是否能正确传入被调函数；
/// - 验证被调函数循环更新后，返回值区间是否能回传给调用点；
/// - 验证 inter-procedural（跨过程）分析路径。
pub fn interprocedural_ranges() -> usize {
    fn foo1(mut k: usize) {
        while k < 100 {
            k += 1;
        }
    }
    fn foo2(mut k: usize, _c: usize) -> usize {
        while k < 100 {
            k += 1;
        }
        k
    }

    foo1(42);
    foo2(55, 52)
}

/// testcase: 分支路径约束（对应 range_3）
///
/// 测试功能：
/// - 验证 if/else 多分支中的路径条件约束；
/// - 验证不同路径上变量更新后，合流点的区间合并行为；
/// - 覆盖“嵌套分支 + 比较条件”的约束推理。
pub fn path_constraints_branching() {
    let x = 1i32;
    let mut y = 10i32;
    let z = 0i32;
    if x < y {
        y += 1;
    } else {
        y -= 1;
        if y > z {
            y -= 2;
        } else {
            y += 2;
        }
    }
    let _ = y;
}

/// testcase: 递归路径（对应 range_4）
///
/// 测试功能：
/// - 验证递归调用中的参数递减关系；
/// - 验证终止分支（`n <= 0`）与递归分支的路径分离；
/// - 覆盖“函数自调用”下的区间稳定性。
pub fn recursion_countdown(n: i32) -> i32 {
    if n <= 0 {
        0
    } else {
        recursion_countdown(n - 1)
    }
}

/// testcase: 符号表达式区间（对应 range_symbolic）
///
/// 测试功能：
/// - 验证分析器对 `x + 1`、`z - y` 这类符号关系的建模；
/// - 验证在“条件恒真（y > x）”场景下，分支可达性与结果区间；
/// - 覆盖 symbolic interval 的表达能力。
pub fn symbolic_interval_case(x: i32) -> i32 {
    let y = x + 1;
    let z = y + 1;
    if y > x {
        z - y
    } else {
        z - x
    }
}

// ============================================================================
// 二、数组 / slice 长度与索引范围
// ============================================================================

/// testcase: for + slice.len()（对应 range_array）
///
/// 测试功能：
/// - 验证由切片构造 `&mut a[1..slice_index]` 推导出的长度；
/// - 验证 `for i in 0..slice.len()` 下 `slice[i]` 的索引范围安全；
/// - 覆盖“切片长度驱动循环上界”的常见模式。
pub fn slice_len_for_loop() {
    let mut a = [0usize; 10];
    let slice_index = 5usize;
    let slice = &mut a[1..slice_index];
    for i in 0..slice.len() {
        slice[i] = i + 1;
    }
}

/// testcase: while + slice.len() + 非线性更新（对应 range_6）
///
/// 测试功能：
/// - 验证 while 条件 `i < 2 * len` 对区间的约束；
/// - 覆盖循环体内乘法与条件更新导致的“非线性区间传播”；
/// - 用于观察分析在复杂更新路径下的保守性。
pub fn slice_len_while_non_linear() {
    let pieces = [42usize; 8];
    let slice_index = 8usize;
    let slice = &pieces[1..slice_index];
    let len = slice.len();
    let mut i = 0usize;

    while i < 2 * len {
        let mut val = slice[i];
        if val > 128 {
            val += 1;
            i *= 2;
            i += 2;
        } else {
            i *= 21;
        }
        let _ = val;
    }
}

/// testcase: 双数组与子切片索引（对应 range_array2）
///
/// 测试功能：
/// - 验证从切片 `x[1..9]` 推导出的合法索引区间；
/// - 验证多数组并行访问中同一循环变量 `i` 的范围复用；
/// - 覆盖“基于切片长度驱动访问原数组”的边界行为。
pub fn dual_array_slice_indexing() {
    let mut x = [0usize; 10];
    let mut y = [0usize; 10];
    let z = &mut x[1..9];
    for i in 0..z.len() {
        x[i] += 1;
        y[i] += 1;
    }
}

/// testcase: 字符串/字节索引与字符区间（对应 range_5）
///
/// 测试功能：
/// - 验证 `string.as_bytes()[0]` 的字节索引模式；
/// - 验证 `char_indices` 与分支匹配下的索引切片 `input[..i]` / `input[i+1..]`；
/// - 覆盖“字符分类 + 索引切分”的真实场景。
pub fn parse_scheme_case(input: &str, context: bool) -> Option<(String, &str)> {
    #[derive(PartialEq, Eq)]
    enum Context {
        UrlParser,
        Setter,
    }

    #[inline]
    fn starts_with_ascii_alpha(string: &str) -> bool {
        matches!(string.as_bytes()[0], b'a'..=b'z' | b'A'..=b'Z')
    }

    if input.is_empty() || !starts_with_ascii_alpha(input) {
        return None;
    }

    for (i, c) in input.char_indices() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '+' | '-' | '.' => (),
            ':' => return Some((input[..i].to_ascii_lowercase(), &input[i + 1..])),
            _ => return None,
        }
    }

    let mode = if context {
        Context::Setter
    } else {
        Context::UrlParser
    };

    match mode {
        Context::Setter => Some((input.to_ascii_lowercase(), "")),
        Context::UrlParser => None,
    }
}

/// testcase: 对齐/重解释切片（对应 range_align）
///
/// 测试功能：
/// - 验证 `from_raw_parts_mut` 构造后固定区间循环访问（0..20）；
/// - 覆盖不安全转换后的索引约束；
/// - 用于观察分析在 unsafe 场景下的区间处理。
pub fn align_and_reinterpret_slice(a: &mut [u8], b: &[u32; 20]) {
    unsafe {
        let c = slice::from_raw_parts_mut(a.as_mut_ptr() as *mut u32, 20);
        for i in 0..20 {
            c[i] ^= b[i];
        }
    }
}

// ============================================================================
// 三、BCE（Bounds Check Elimination）中 LLVM 难以消除的场景
// ============================================================================

/// testcase: 间接索引（BCE failure）
///
/// 测试功能：
/// - `idx` 来自 `indices[i]` 的数据依赖；
/// - LLVM 无法静态证明 `data[idx]` 总是安全；
/// - 预期保留边界检查。
pub fn bce_failure_indirect_indexing(data: &[i32], indices: &[usize]) -> i32 {
    let mut sum = 0;
    for i in 0..indices.len() {
        let idx = indices[i];
        sum += data[idx];
    }
    sum
}

/// testcase: 循环内修改容器长度（BCE failure）
///
/// 测试功能：
/// - 在循环内 `push` 导致长度可能变化；
/// - 编译器难以复用先前的边界证明；
/// - 预期 `v[i]` 访问难以完全消除检查。
pub fn bce_failure_mutation_invalidation(v: &mut Vec<i32>) {
    let len = v.len();
    for i in 0..len {
        let val = v[i];
        v.push(val * 2);
    }
}

/// testcase: 复杂步长归纳变量（BCE failure）
///
/// 测试功能：
/// - `step_by(dynamic_step)` 的动态步长使归纳证明复杂化；
/// - 编译器通常无法稳定证明每次访问都满足边界；
/// - 预期 `slice[i]` 访问常保留检查。
pub fn bce_failure_complex_induction(slice: &[i32], dynamic_step: usize) -> i32 {
    if dynamic_step == 0 {
        return 0;
    }
    let mut sum = 0;
    for i in (0..slice.len()).step_by(dynamic_step) {
        sum += slice[i];
    }
    sum
}

#[inline(never)]
fn get_opaque_index() -> usize {
    42
}

/// testcase: 非内联边界函数返回索引（BCE failure）
///
/// 测试功能：
/// - 索引来自 `#[inline(never)]` 函数，局部信息不透明；
/// - 调用点难以做跨边界值域推断；
/// - 预期 `slice[idx]` 边界检查被保留。
pub fn bce_failure_opaque_boundary(slice: &[i32]) -> i32 {
    let idx = get_opaque_index();
    slice[idx]
}
