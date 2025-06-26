//! Dominance and dominance frontier calculation.
use std::collections::VecDeque;

use crate::mem2reg::Mem2RegFn;
use bit_vec::BitVec;
use koopa::ir::*;

impl Mem2RegFn {
    pub fn init_dom(&mut self, prog: &Program, f: Function) {
        let f_data = prog.func(f);
        let dfg = f_data.dfg();

        // List BBs
        let entry_bb = f_data.layout().entry_bb().unwrap();
        self.bb_list.push(entry_bb);
        eprintln!("entry_bb: {:?}", entry_bb);
        for (b, _) in dfg.bbs().iter() {
            if *b != entry_bb {
                eprintln!("bb id {}: {:?}", self.bb_list.len(), b);
                self.bb_list.push(*b);
            }
        }

        let n_bb = self.n_bb();

        self.pred = vec![vec![]; n_bb];
        self.succ = vec![vec![]; n_bb];
        for (i, b) in self.bb_list.iter().enumerate() {
            // The mapped BB is a placeholder.
            self.bbs.insert(*b, (i, *b));
        }

        // Process DFG into a easier form.
        for (i, b) in self.bb_list.iter().enumerate() {
            let bbn = f_data.layout().bbs().node(&b).unwrap();
            let exit_insn = bbn.insts().back_key().unwrap().clone();
            match dfg.value(exit_insn).kind() {
                ValueKind::Branch(branch) => {
                    let true_bb = self.bbs.get(&branch.true_bb()).unwrap().0;
                    let false_bb = self.bbs.get(&branch.false_bb()).unwrap().0;
                    self.succ[i] = vec![true_bb, false_bb];
                    self.pred[true_bb].push(i);
                    self.pred[false_bb].push(i);
                }
                ValueKind::Jump(jump) => {
                    let target_bb = self.bbs.get(&jump.target()).unwrap().0;
                    self.succ[i].push(target_bb);
                    self.pred[target_bb].push(i);
                }
                ValueKind::Return(_) => (),
                _ => panic!("BB does not pass control flow"),
            }
        }

        // Calculate dom.
        // Can be implemented O(n^2), but bit_vec is fast and straightforward.
        self.dom = vec![BitVec::from_elem(n_bb, true); n_bb];
        self.dom[0].clear();
        self.dom[0].set(0, true);
        {
            let mut q = VecDeque::new();
            q.push_back(0_usize);
            while !q.is_empty() {
                let u = q.pop_front().unwrap();
                for v in &self.succ[u] {
                    if *v == u {
                        continue;
                    }
                    let mut tmp = self.dom[u].clone();
                    tmp.set(*v, true);
                    let changed = self.dom[*v].and(&tmp);
                    if changed {
                        q.push_back(*v);
                    }
                }
            }
        }

        eprintln!("Dom: {:?}", self.dom);

        // Calculate DF.
        self.df = vec![BitVec::from_elem(n_bb, false); n_bb];
        for u in 0..n_bb {
            for v in 0..n_bb {
                if !self.dom[v].get(u).unwrap() {
                    for p in &self.pred[v] {
                        if self.dom[*p].get(u).unwrap() {
                            self.df[u].set(v, true);
                            break;
                        }
                    }
                }
            }
        }
        eprintln!("DF: {:?}", self.df);
    }
}
