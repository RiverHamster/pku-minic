use bit_vec::BitVec;
use std::collections::HashMap;

use super::irpass::IRPass;
use koopa::ir::*;

mod alloc;
mod convert;
mod dom;
pub struct Mem2Reg;

struct Mem2RegFn {
    // Maps original BBs to BBs with arguments and linear id.
    bbs: HashMap<BasicBlock, (usize, BasicBlock)>,
    // Linearly indexed BBs.
    bb_list: Vec<BasicBlock>,
    // Predecessors and successors of a BB.
    // If there are two successors, the first is true_bb, second is false_bb.
    pred: Vec<Vec<usize>>,
    succ: Vec<Vec<usize>>,
    // Maps SSA-lifted allocs to linear id.
    lifted_alloc: HashMap<Value, usize>,
    // The types underlying the lifted allocs, like i32 or *[i32, N].
    alloc_ty: Vec<Type>,
    // The BB arguments (alloc IDs) for each BB.
    bb_args: Vec<Vec<usize>>,
    // Dominators of a BB.
    dom: Vec<BitVec>,
    // Dominance frontier of a BB.
    df: Vec<BitVec>,
}

impl Mem2RegFn {
    fn new() -> Self {
        Mem2RegFn {
            bbs: HashMap::new(),
            bb_list: vec![],
            pred: vec![],
            succ: vec![],
            lifted_alloc: HashMap::new(),
            alloc_ty: vec![],
            bb_args: vec![],
            dom: vec![],
            df: vec![],
        }
    }

    fn n_bb(&self) -> usize {
        self.bb_list.len()
    }

    fn mem2reg(&mut self, prog: &mut Program, f: Function) {
        if prog.func(f).layout().entry_bb().is_none() {
            // f is a declaration.
            return;
        }
        // eprintln!("=== Translate function {:?} ===", f);
        self.init_dom(prog, f);
        self.get_bb_args(prog, f);
        self.convert(prog, f);
        // for (_i, (bb, node)) in prog.func(f).layout().bbs().iter().enumerate() {
        //     eprintln!("bb {:?}: # of insts {}", bb, node.insts().len());
        // }
    }
}

impl IRPass for Mem2Reg {
    fn run(&mut self, prog: &mut Program) {
        let funcs: Vec<_> = prog.func_layout().iter().cloned().collect();
        for f in funcs {
            let mut converter = Mem2RegFn::new();
            converter.mem2reg(prog, f);
        }
    }
}
