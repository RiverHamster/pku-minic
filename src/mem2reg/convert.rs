//! IR conversion based on alloc and BB argument information.

use std::collections::HashMap;

use bit_vec::BitVec;
use koopa::ir::{
    builder::{BasicBlockBuilder, LocalInstBuilder, ValueBuilder},
    *,
};

use crate::mem2reg::Mem2RegFn;

impl Mem2RegFn {
    /// Convert BB b into the SSA form, and iterate over the successors of b.
    /// The DFS is used for maintaining the `alloc_val` mapping.
    /// Since the definition of SSA values must dominate its uses, any DFS order
    /// is sufficient for finding the definition of the values.
    fn dfs(
        &self,
        f_data: &mut FunctionData,
        b: BasicBlock,
        mut ssa_val: Vec<Option<Value>>,
        mut load_subst: HashMap<Value, Value>,
        vis: &mut BitVec,
    ) {
        macro_rules! add_insn {
            ($bb: expr, $v:expr) => {
                eprintln!("add_insn {:?} {:?}", $bb, $v);
                f_data.layout_mut().bb_mut($bb).insts_mut().extend([$v]);
                // eprintln!("# of insts: {}", f_data.layout_mut().bb_mut($bb).insts().len());
            };
        }
        macro_rules! ssa {
            ($v:expr) => {
                if let Some(sv) = load_subst.get(&$v) {
                    *sv
                } else {
                    $v
                }
            };
        }
        macro_rules! revalue {
            ($v:expr) => {
                f_data.dfg_mut().replace_value_with($v)
            };
        }

        eprintln!("dfs BB {:?}", b);

        let (b_id, arg_bb) = self.bbs.get(&b).cloned().unwrap();
        if vis.get(b_id).unwrap() {
            return;
        }
        vis.set(b_id, true);

        // BB params (PHI nodes) are SSA values.
        let b_data = f_data.dfg().bb(arg_bb);
        for (i, v_id) in self.bb_args[b_id].iter().enumerate() {
            ssa_val[*v_id] = Some(b_data.params()[i]);
        }

        let _arg_bb_data = f_data.dfg().bb(arg_bb);

        let insts: Vec<_> = f_data
            .layout()
            .bbs()
            .node(&b)
            .unwrap()
            .insts()
            .iter()
            .map(|(v, _)| v.clone())
            .collect();
        for v in insts {
            let vk = f_data.dfg().value(v).kind().clone();
            eprintln!("translate value {:?}", vk);
            use ValueKind::*;
            match vk {
                Integer(_) | ZeroInit(_) | Undef(_) | Aggregate(_) | FuncArgRef(_) => {
                    add_insn!(arg_bb, v);
                }
                // Memory accesses.
                // Replace with SSA values if applicable. Otherwise, add the instruction as-is.
                Alloc(_) => {
                    if !self.lifted_alloc.contains_key(&v) {
                        add_insn!(arg_bb, v);
                    }
                }
                Load(l) => {
                    if let Some(alloc_id) = self.lifted_alloc.get(&l.src()) {
                        // Substitute future uses of this load with the SSA value.
                        load_subst.insert(v, ssa_val[*alloc_id].unwrap());
                    } else {
                        add_insn!(arg_bb, v);
                    }
                }
                Store(s) => {
                    if let Some(alloc_id) = self.lifted_alloc.get(&s.dest()) {
                        // Update the SSA value.
                        eprintln!("Update SSA {}", alloc_id);
                        ssa_val[*alloc_id] = Some(ssa!(s.value()));
                    } else {
                        revalue!(v).store(ssa!(s.value()), s.dest());
                        add_insn!(arg_bb, v);
                    }
                }
                // Operations.
                // Replace the loads involved with SSA values.
                GetPtr(g) => {
                    revalue!(v).get_ptr(ssa!(g.src()), ssa!(g.index()));
                    add_insn!(arg_bb, v);
                }
                GetElemPtr(g) => {
                    revalue!(v).get_elem_ptr(ssa!(g.src()), ssa!(g.index()));
                    add_insn!(arg_bb, v);
                }
                Binary(b) => {
                    revalue!(v).binary(b.op(), ssa!(b.lhs()), ssa!(b.rhs()));
                    add_insn!(arg_bb, v);
                }
                Call(c) => {
                    revalue!(v).call(c.callee(), c.args().iter().map(|a| ssa!(*a)).collect());
                    add_insn!(arg_bb, v);
                }
                // Control flow.
                // Add the BB arguments.
                Branch(b) => {
                    eprintln!(
                        "Branch: true_bb {:?}, false_bb {:?}",
                        b.true_bb(),
                        b.false_bb()
                    );
                    let (true_id, true_bb) = self.bbs.get(&b.true_bb()).cloned().unwrap();
                    let (false_id, false_bb) = self.bbs.get(&b.false_bb()).cloned().unwrap();
                    let true_args = self.bb_args[true_id]
                        .iter()
                        .map(|v_id| ssa_val[*v_id].unwrap())
                        .collect();
                    let false_args = self.bb_args[false_id]
                        .iter()
                        .map(|v_id| ssa_val[*v_id].unwrap())
                        .collect();
                    revalue!(v).branch_with_args(
                        ssa!(b.cond()),
                        true_bb,
                        false_bb,
                        true_args,
                        false_args,
                    );
                    add_insn!(arg_bb, v);
                    self.dfs(
                        f_data,
                        b.true_bb(),
                        ssa_val.clone(),
                        load_subst.clone(),
                        vis,
                    );
                    self.dfs(f_data, b.false_bb(), ssa_val, load_subst, vis);
                    break;
                }
                Jump(j) => {
                    let (t_id, t_bb) = self.bbs.get(&j.target()).cloned().unwrap();
                    eprintln!("target_id {}, required args {:?}", t_id, self.bb_args[t_id]);
                    // Sometimes it is inevitable that we add BB args that are impossible to
                    // fill at some times... Such an argument must be i32, so we always pass
                    // zero.
                    // See README.md for examples.
                    let args = self.bb_args[t_id]
                        .iter()
                        .map(|v_id| {
                            ssa_val[*v_id].unwrap_or_else(|| {
                                f_data
                                    .dfg_mut()
                                    .new_value()
                                    .undef(self.alloc_ty[*v_id].clone())
                            })
                        })
                        .collect();
                    eprintln!("args {:?}", args);
                    revalue!(v).jump_with_args(t_bb, args);
                    add_insn!(arg_bb, v);
                    self.dfs(f_data, j.target(), ssa_val, load_subst, vis);
                    break;
                }
                Return(r) => {
                    revalue!(v).ret(r.value().map(|v| ssa!(v)));
                    add_insn!(arg_bb, v);
                    break;
                }
                // Invalid.
                BlockArgRef(_) => panic!("mem2reg does not support BB args"),
                GlobalAlloc(_) => unreachable!("global_alloc in local values"),
            }
        }
    }

    pub fn convert(&mut self, prog: &mut Program, f: Function) {
        // Create BB with arguments corresponding to each BB.
        let f_data = prog.func_mut(f);
        // let dfg = f_data.dfg_mut();
        let n_bb = self.n_bb();
        for (b_id, b) in self.bb_list.iter().enumerate() {
            let params_ty = self.bb_args[b_id]
                .iter()
                .map(|v_id| self.alloc_ty[*v_id].clone())
                .collect();
            let name = f_data.dfg().bb(*b).name().as_ref().unwrap().clone();
            let arg_bb = f_data
                .dfg_mut()
                .new_bb()
                .basic_block_with_params(Some(name + "SSA"), params_ty);
            f_data.layout_mut().bbs_mut().extend([arg_bb]);
            self.bbs.get_mut(b).unwrap().1 = arg_bb;
        }

        let entry_bb = f_data.layout_mut().entry_bb().unwrap();
        let entry_bb_arg = self.bbs.get(&entry_bb).unwrap().1;
        let mut vis = BitVec::from_elem(n_bb, false);
        self.dfs(
            f_data,
            entry_bb,
            vec![None; self.lifted_alloc.len()],
            HashMap::new(),
            &mut vis,
        );

        // Replace all BBs with BBs with arguments.
        for (b, _) in &self.bbs {
            f_data.dfg_mut().remove_bb(*b);
            f_data.layout_mut().bbs_mut().remove(b);
        }

        // let knl = f_data.layout_mut().bbs_mut();
        // eprintln!("transformed entry: {:?}", entry_bb_arg);
        // TODO: is it required to manually move the entry BB? Moving get all the insns lost?
        // let entry_node = knl.remove(&entry_bb_arg).unwrap();
        // eprintln!("# of entry BB insns: {}", entry_node.1.insts().len());
        // if let Err(_) = knl.push_front(entry_node.0, entry_node.1) {
        //     panic!("Duplicate key, the key should be removed");
        // }
        assert!(f_data.layout().entry_bb().unwrap() == entry_bb_arg);
    }
}
