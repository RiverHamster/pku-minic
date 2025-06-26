//! Mark the allocs for lifting and basic block arguments.
use bit_vec::BitVec;
use koopa::ir::*;

use crate::mem2reg::Mem2RegFn;

impl Mem2RegFn {
    pub fn get_bb_args(&mut self, prog: &Program, f: Function) {
        let f_data = prog.func(f);
        let dfg = f_data.dfg();
        let n_bb = self.n_bb();

        // Find candidate allocs.
        for (v, vd) in dfg.values() {
            if let (ValueKind::Alloc(_), TypeKind::Pointer(p)) = (vd.kind(), vd.ty().kind()) {
                if p.is_i32() || matches!(p.kind(), TypeKind::Pointer(_)) {
                    let v_id: usize = self.lifted_alloc.len();
                    self.lifted_alloc.insert(*v, v_id);
                    self.alloc_ty.push(p.clone());
                }
            }
        }

        // For each BB, record the allocs it gives a definition.
        let mut def_bb = vec![BitVec::from_elem(n_bb, false); self.lifted_alloc.len()];
        for (b_id, b) in self.bb_list.iter().enumerate() {
            let insns = f_data.layout().bbs().node(b).unwrap().insts();
            for (v, _) in insns {
                let v_data = dfg.value(*v);
                let dest = match v_data.kind() {
                    ValueKind::Store(s) => s.dest(),
                    // ValueKind::Alloc(_) => *v,
                    _ => continue,
                };
                if let Some(v_id) = self.lifted_alloc.get(&dest) {
                    def_bb[*v_id].set(b_id, true);
                }
            }
        }

        let mut alloc_at_bb = vec![n_bb; self.lifted_alloc.len()];
        for (b, (b_id, _)) in &self.bbs {
            for (v, _) in f_data.layout().bbs().node(b).unwrap().insts() {
                let v_id = self.lifted_alloc.get(v);
                if let Some(v_id) = v_id {
                    alloc_at_bb[*v_id] = *b_id;
                }
            }
        }

        self.bb_args = vec![vec![]; n_bb];
        for (_, v_id) in &self.lifted_alloc {
            let mut df_union = BitVec::from_elem(n_bb, false);
            for b_id in 0..n_bb {
                if def_bb[*v_id].get(b_id).unwrap() {
                    df_union.or(&self.df[b_id]);
                }
            }
            loop {
                let mut changed = false;
                for b_id in 0..n_bb {
                    if df_union.get(b_id).unwrap() {
                        changed |= df_union.or(&self.df[b_id]);
                    }
                }
                if !changed {
                    break;
                }
            }
            for b_id in 0..n_bb {
                // The alloc must dominate the BB, otherwise some predecessors may
                // not be able to provide the block argument.
                // TODO: This can be refined as: the alloc is live at this BB.
                //
                // Example:
                // if (cond) { int b = 1; } ...
                // b should not be added as any block argument. But it does have
                // dominance frontier.
                if df_union.get(b_id).unwrap()
                    && self.dom[b_id].get(alloc_at_bb[*v_id]).unwrap()
                    && alloc_at_bb[*v_id] != b_id
                {
                    self.bb_args[b_id].push(*v_id);
                }
            }
        }

        for args in &mut self.bb_args {
            args.sort();
        }
        for (i, args) in self.bb_args.iter().enumerate() {
            eprintln!("bb {}: args {:?}", i, args);
        }
    }
}
