use core::panic;
use koopa::ir::{dfg::DataFlowGraph, *};
use std::{collections::HashMap, io};

struct SimpleRISCVBuilder<W: io::Write> {
    writer: W,
}

/// calculate the (stack size, size of values) of a function
fn stack_size(prog: &Program, f: Function) -> (usize, usize) {
    prog.func(f)
        .dfg()
        .values()
        .iter()
        .fold((0_usize, 0_usize), |(stack_size, val_size), (_, v)| {
            let vsize = v.ty().size();
            let valloc = if matches!(v.kind(), ValueKind::Alloc(_)) {
                match v.ty().kind() {
                    TypeKind::Pointer(t) => t.size(),
                    _ => panic!("alloc value must be a pointer"),
                }
            } else {
                0
            };
            (stack_size + vsize + valloc, val_size + vsize)
        })
}

struct StackManager {
    offsets: HashMap<Value, usize>,
    bound: usize,
}

impl StackManager {
    fn new(base: usize) -> Self {
        Self {
            offsets: HashMap::new(),
            bound: base,
        }
    }

    fn get(&mut self, v: Value, dfg: &DataFlowGraph) -> usize {
        // increment the bound to allocate, or return the value
        self.offsets.get(&v).copied().unwrap_or_else(|| {
            let pos = self.bound;
            self.bound += dfg.value(v).ty().size();
            self.offsets.insert(v, pos);
            pos
        })
    }
}

impl<W: io::Write> SimpleRISCVBuilder<W> {
    fn new(writer: W) -> Self {
        Self { writer }
    }

    fn add_func(&mut self, prog: &Program, f_handle: Function) {
        let f = prog.func(f_handle);
        let dfg = f.dfg();
        let f_name = &f.name()[1..];
        writeln!(self.writer, "  .globl {}\n{}:", f_name, f_name).unwrap();

        let (stack_size, val_size) = stack_size(prog, f_handle);
        writeln!(self.writer, "  addi sp, sp, -{}", stack_size).unwrap();

        let mut stk_val = StackManager::new(0);
        let mut stk_var = StackManager::new(val_size);

        for (val, val_data) in dfg.values() {
            if let ValueKind::Integer(i) = val_data.kind() {
                let pos = stk_val.get(*val, dfg);
                writeln!(self.writer, "  li t0, {}", i.value()).unwrap();
                writeln!(self.writer, "  sw t0, {pos}(sp)").unwrap();
            }
        }

        let bbs = f.layout().bbs();
        for (bb, bbn) in bbs {
            let bb_name = dfg.bb(*bb).name().as_ref().unwrap();
            writeln!(self.writer, "L{}:", &bb_name[1..]).unwrap();
            for (val_handle, inst_node) in bbn.insts() {
                let val = dfg.value(*val_handle);
                match val.kind() {
                    ValueKind::Integer(i) => {
                        writeln!(self.writer, "  li t0, {}", i.value()).unwrap();
                    }
                    // pre-allocated
                    ValueKind::Alloc(_) => {}
                    ValueKind::Load(l) => {
                        let src_off = stk_var.get(l.src(), dfg);
                        let pos = stk_val.get(*val_handle, dfg);
                        writeln!(self.writer, "  lw t0, {src_off}(sp)").unwrap();
                        writeln!(self.writer, "  sw t0, {pos}(sp)").unwrap();
                    }
                    ValueKind::Store(s) => {
                        let src_off = stk_val.get(s.value(), dfg);
                        let dst_off = stk_var.get(s.dest(), dfg);
                        writeln!(self.writer, "  lw t0, {src_off}(sp)").unwrap();
                        writeln!(self.writer, "  sw t0, {dst_off}(sp)").unwrap();
                    }
                    ValueKind::Return(r) => {
                        if let Some(v) = r.value() {
                            let pos = stk_val.get(v, dfg);
                            writeln!(self.writer, "  lw a0, {pos}(sp)").unwrap();
                        }
                        writeln!(self.writer, "  addi sp, sp, {}", stack_size).unwrap();
                        writeln!(self.writer, "  ret").unwrap();
                    }
                    ValueKind::Binary(b) => {
                        // load the operands
                        let lhs = b.lhs();
                        let rhs = b.rhs();
                        let lhs_off = stk_val.get(lhs, dfg);
                        let rhs_off = stk_val.get(rhs, dfg);
                        writeln!(self.writer, "  lw t0, {lhs_off}(sp)").unwrap();
                        writeln!(self.writer, "  lw t1, {rhs_off}(sp)").unwrap();

                        // perform the operation
                        let insns = match b.op() {
                            BinaryOp::NotEq => "xor t0, t0, t1\nsnez t0, t0",
                            BinaryOp::Eq => "xor t0, t0, t1\nseqz t0, t0",
                            BinaryOp::Gt => "slt t0, t1, t0",
                            BinaryOp::Lt => "slt t0, t0, t1",
                            BinaryOp::Ge => "slt t0, t0, t1\nxori t0, t0, 1",
                            BinaryOp::Le => "slt t0, t1, t0\nxori t0, t0, 1",
                            BinaryOp::Add => "add t0, t0, t1",
                            BinaryOp::Sub => "sub t0, t0, t1",
                            BinaryOp::Mul => "mul t0, t0, t1",
                            BinaryOp::Div => "div t0, t0, t1",
                            BinaryOp::Mod => "rem t0, t0, t1",
                            BinaryOp::And => "and t0, t0, t1",
                            BinaryOp::Or => "or t0, t0, t1",
                            BinaryOp::Xor => "xor t0, t0, t1",
                            BinaryOp::Shl => "sll t0, t0, t1",
                            BinaryOp::Shr => "srl t0, t0, t1",
                            BinaryOp::Sar => "sra t0, t0, t1",
                        };
                        writeln!(self.writer, "  {}", insns).unwrap();

                        // write the value to the stack
                        let pos = stk_val.get(*val_handle, dfg);
                        writeln!(self.writer, "  sw t0, {}(sp)", pos).unwrap();
                    }
                    _ => unimplemented!("value kind {:?} not implemented", val.kind()),
                }
            }
        }
    }

    fn add_global_vars(&mut self, prog: &Program) {
        // TODO
    }

    pub fn add_program(&mut self, prog: &Program) {
        self.add_global_vars(prog);

        writeln!(self.writer, "  .text").unwrap();
        for f_handle in prog.func_layout() {
            self.add_func(prog, *f_handle);
        }
    }
}

pub fn gen_riscv_simple<W: io::Write>(prog: &Program, writer: W) {
    let mut builder = SimpleRISCVBuilder::new(writer);
    builder.add_program(prog);
}
