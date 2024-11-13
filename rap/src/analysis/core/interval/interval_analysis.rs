use rustc_middle::ty::TyCtxt;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::cell::RefCell;

use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Value;


struct VarNode {
    value: Rc<Value>,
}

impl VarNode {

}


struct BasicOp;
struct APInt;
struct Instruction;
struct Function;
struct PHINode;
struct BranchInst;
struct SwitchInst;
struct Range;

impl VarNode {
    fn new(value: Rc<Value>) -> Self {
        VarNode { value }
    }
}

// ConstraintGraph 结构体
struct ConstraintGraph {
    // 变量节点映射和操作节点集合
    vars: HashMap<Rc<Value>, Rc<RefCell<VarNode>>>,
    oprs: HashSet<BasicOp>,

    // 私有字段
    func: Option<Rc<Function>>,
    def_map: HashMap<Rc<Value>, BasicOp>,
    use_map: HashMap<Rc<Value>, HashSet<Rc<BasicOp>>>,
    symb_map: HashMap<Rc<Value>, HashSet<Rc<BasicOp>>>,
    values_branch_map: HashMap<Rc<Value>, Vec<BasicOp>>,
    values_switch_map: HashMap<Rc<Value>, Vec<BasicOp>>,
    constant_vector: Vec<APInt>,
}

impl ConstraintGraph {
    fn new() -> Self {
        ConstraintGraph {
            vars: HashMap::new(),
            oprs: HashSet::new(),
            func: None,
            def_map: HashMap::new(),
            use_map: HashMap::new(),
            symb_map: HashMap::new(),
            values_branch_map: HashMap::new(),
            values_switch_map: HashMap::new(),
            constant_vector: Vec::new(),
        }
    }

    fn add_var_node(&mut self, v: Rc<Value>) -> Rc<RefCell<VarNode>> {
        if let Some(node) = self.vars.get(&v) {
            return Rc::clone(node);
        }
        let node = Rc::new(RefCell::new(VarNode::new(Rc::clone(&v))));
        self.vars.insert(Rc::clone(&v), Rc::clone(&node));
        node
    }

    fn add_binary_op(&mut self, _inst: &Instruction) {
        // TODO: Implement binary op addition logic
    }

    fn add_phi_op(&mut self, _phi: &PHINode) {
        // TODO: Implement phi op addition logic
    }

    fn add_sigma_op(&mut self, _sigma: &PHINode) {
        // TODO: Implement sigma op addition logic
    }

    fn build_operations(&mut self, _inst: &Instruction) {
        // TODO: Implement operations building logic
    }

    fn build_value_branch_map(&mut self, _br: &BranchInst) {
        // TODO: Implement branch map building logic
    }

    fn build_value_switch_map(&mut self, _sw: &SwitchInst) {
        // TODO: Implement switch map building logic
    }

    fn build_value_maps(&mut self, _func: &Function) {
        // TODO: Implement value maps building logic
    }

    fn insert_constant_into_vector(&mut self, constant_val: APInt) {
        self.constant_vector.push(constant_val);
    }

    // fn get_first_greater_from_vector(&self, val: &APInt) -> Option<&APInt> {

    // }

    // fn get_first_less_from_vector(&self, val: &APInt) -> Option<&APInt> {

    // }

    fn build_constant_vector(&mut self, component: &HashSet<Rc<RefCell<VarNode>>>, compusemap: &HashMap<Rc<Value>, HashSet<Rc<BasicOp>>>) {
        // TODO: Implement constant vector building logic
    }

    fn update(&mut self, comp_use_map: &HashMap<Rc<Value>, HashSet<Rc<BasicOp>>>, actv: &mut HashSet<Rc<Value>>) {
        // TODO: Implement update logic
    }

    fn clear(&mut self) {
        // Clear the graph's data
        self.vars.clear();
        self.oprs.clear();
        self.def_map.clear();
        self.use_map.clear();
        self.symb_map.clear();
        self.constant_vector.clear();
    }

    fn print(&self, _func: &Function) {
        // TODO: Implement printing logic
    }

    fn dump(&self, func: &Function) {
        self.print(func);
        // Add additional debug information if needed
    }

    fn compute_stats(&self) {
        // TODO: Implement statistics computation
    }

    fn get_range(&self, _v: &Value) -> Option<Range> {
        // TODO: Implement range retrieval
        None
    }
}
