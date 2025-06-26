use core::panic;
use koopa::ir::*;
use std::{collections::HashMap, io};

use stack::{load_stack, stack_size, write_stack, StackManager};
use util::{align, flatten_aggregate_const};

use crate::codegen::{
    stack::{load_addr, load_val, store_val},
    util::{flatten_local_aggregate, get_type},
};

mod stack;
mod util;

const RV_ADDI_LIMIT: usize = 2047;
const RV_OFFSET_LIMIT: usize = 2047;
/// RV32
const RV_WORD_SIZE: usize = 4;
const RV_N_ARGREG: usize = 8;

struct SimpleRISCVBuilder<W: io::Write> {
    writer: W,
    long_branch_label: usize,
}

impl<W: io::Write> SimpleRISCVBuilder<W> {
    fn new(writer: W) -> Self {
        Self {
            writer,
            long_branch_label: 0,
        }
    }

    fn add_func(&mut self, prog: &Program, f_handle: Function) {
        let f = prog.func(f_handle);
        if f.dfg().bbs().is_empty() {
            // f is a declaration
            return;
        }
        let dfg = f.dfg();
        let f_name = &f.name()[1..];
        writeln!(self.writer, "  .globl {}\n{}:", f_name, f_name).unwrap();

        let arg_idx: HashMap<Value, usize> =
            HashMap::from_iter(f.params().iter().enumerate().map(|(i, v)| (*v, i)));

        // let (stack_size, val_size) = stack_size(prog, f_handle);
        let stack = stack_size(prog, f_handle);
        let stack_size = align(
            stack.alloc + stack.val + stack.arg_cons + stack.save_regs,
            16,
        );
        // eprintln!(
        //     "add_func {}, stack {:?}, allocation {}",
        //     f.name(),
        //     stack,
        //     stack_size
        // );

        if stack_size > RV_ADDI_LIMIT {
            writeln!(self.writer, "  li t0, -{}", stack_size).unwrap();
            writeln!(self.writer, "  add sp, sp, t0").unwrap();
        } else {
            writeln!(self.writer, "  addi sp, sp, -{}", stack_size).unwrap();
        }

        let ra_off = stack.arg_cons + stack.val + stack.alloc;
        if stack.save_ra {
            write_stack(&mut self.writer, ra_off, "ra");
        }

        // Values are stored on stack.
        // Allocations are typed as pointers, so they are dereferenced when
        // calculating size.
        let mut stk_val = StackManager::new(stack.arg_cons, false);
        let mut stk_var = StackManager::new(stack.arg_cons + stack.val, true);

        // Save the values for function arguments.
        for (i, v) in f.params().iter().enumerate() {
            let val_off = stk_val.get(*v, dfg);
            if i < RV_N_ARGREG {
                write_stack(&mut self.writer, val_off, &format!("a{}", i));
            } else {
                let src_off = RV_WORD_SIZE * (i - RV_N_ARGREG) + stack_size;
                load_stack(&mut self.writer, src_off, "t0");
                write_stack(&mut self.writer, val_off, "t0");
            }
        }

        for (val, val_data) in dfg.values() {
            if let ValueKind::Integer(i) = val_data.kind() {
                let pos = stk_val.get(*val, dfg);
                writeln!(self.writer, "  li t0, {}", i.value()).unwrap();
                write_stack(&mut self.writer, pos, "t0");
            }
        }

        macro_rules! pass_bb_args {
            ($target:expr, $actuals:expr) => {
                let args_formal = dfg.bb($target).params();
                let args_actual = $actuals;
                for (formal, actual) in args_formal.iter().zip(args_actual.iter()) {
                    let f_off = stk_val.get(*formal, dfg);
                    let a_off = stk_val.get(*actual, dfg);
                    load_stack(&mut self.writer, a_off, "t0");
                    write_stack(&mut self.writer, f_off, "t0");
                }
            }
        }

        let bbs = f.layout().bbs();
        for (bb, bbn) in bbs {
            let bb_name = dfg.bb(*bb).name().as_ref().unwrap();
            writeln!(self.writer, "L{}:", &bb_name[1..]).unwrap();
            for (val_handle, _inst_node) in bbn.insts() {
                let val = dfg.value(*val_handle);
                match val.kind() {
                    ValueKind::Integer(i) => {
                        writeln!(self.writer, "  li t0, {}", i.value()).unwrap();
                    }
                    // pre-allocated
                    ValueKind::Alloc(_) => {}
                    ValueKind::Load(l) => {
                        let pos = stk_val.get(*val_handle, dfg);
                        if l.src().is_global() {
                            load_val(
                                &mut self.writer,
                                l.src(),
                                prog,
                                dfg,
                                "t0",
                                &mut stk_val,
                                &mut stk_var,
                            );
                        } else { let src_data = dfg.value(l.src());
                            match src_data.kind() {
                                ValueKind::Alloc(_) => {
                                    let src_off = stk_var.get(l.src(), dfg);
                                    load_stack(&mut self.writer, src_off, "t0");
                                }
                                ValueKind::GetPtr(_) | ValueKind::GetElemPtr(_) => {
                                    load_val(
                                        &mut self.writer,
                                        l.src(),
                                        prog,
                                        dfg,
                                        "t0",
                                        &mut stk_val,
                                        &mut stk_var,
                                    );
                                }
                                _ => panic!("unsupported load source {:?}", src_data.kind()),
                            }
                        }
                        write_stack(&mut self.writer, pos, "t0");
                    }
                    // only Store can access function arguments, guaranteed by the IR generator
                    ValueKind::Store(s) => {
                        if s.dest().is_global() {
                            let src_off = stk_val.get(s.value(), dfg);
                            load_stack(&mut self.writer, src_off, "t0");
                            store_val(
                                &mut self.writer,
                                s.dest(),
                                prog,
                                dfg,
                                "t0",
                                "t2",
                                &mut stk_val,
                                &mut stk_var,
                            );
                        } else {
                            let dest_data = dfg.value(s.dest());
                            match dest_data.kind() {
                                ValueKind::Alloc(_) => {
                                    let dst_off = stk_var.get(s.dest(), dfg);
                                    let idx = arg_idx.get(&s.value());
                                    match idx {
                                        // argument
                                        Some(i) => {
                                            if *i < RV_N_ARGREG {
                                                write_stack(
                                                    &mut self.writer,
                                                    dst_off,
                                                    &format!("a{}", i),
                                                );
                                            } else {
                                                let src_off =
                                                    RV_WORD_SIZE * (*i - RV_N_ARGREG) + stack_size;
                                                load_stack(&mut self.writer, src_off, "t0");
                                                write_stack(&mut self.writer, dst_off, "t0");
                                            }
                                        }
                                        // local value (on stack)
                                        None => {
                                            let s_value_data = dfg.value(s.value());
                                            match s_value_data.kind() {
                                                ValueKind::Aggregate(_) => {
                                                    // We assume `dest` is Alloc.
                                                    let vals =
                                                        flatten_local_aggregate(dfg, s.value());
                                                    for (i, v) in vals.iter().enumerate() {
                                                        let v_data = if v.is_global() {
                                                            prog.borrow_value(*v).clone()
                                                        } else {
                                                            dfg.value(*v).clone()
                                                        };
                                                        assert!(v_data.ty().is_i32());
                                                        load_val(
                                                            &mut self.writer,
                                                            *v,
                                                            prog,
                                                            dfg,
                                                            "t0",
                                                            &mut stk_val,
                                                            &mut stk_var,
                                                        );
                                                        write_stack(
                                                            &mut self.writer,
                                                            dst_off + i * RV_WORD_SIZE,
                                                            "t0",
                                                        );
                                                    }
                                                }
                                                _ => {
                                                    // We does not type-check here.
                                                    let src_off = stk_val.get(s.value(), dfg);
                                                    load_stack(&mut self.writer, src_off, "t0");
                                                    write_stack(&mut self.writer, dst_off, "t0");
                                                }
                                            };
                                        }
                                    }
                                }
                                ValueKind::GetPtr(_) | ValueKind::GetElemPtr(_) => {
                                    let src_off = stk_val.get(s.value(), dfg);
                                    load_stack(&mut self.writer, src_off, "t0");
                                    store_val(
                                        &mut self.writer,
                                        s.dest(),
                                        prog,
                                        dfg,
                                        "t0",
                                        "t2",
                                        &mut stk_val,
                                        &mut stk_var,
                                    );
                                }
                                _ => panic!("unsupported store destination {:?}", dest_data.kind()),
                            }
                        }
                    }
                    ValueKind::Return(r) => {
                        if let Some(v) = r.value() {
                            let pos = stk_val.get(v, dfg);
                            load_stack(&mut self.writer, pos, "a0");
                        }

                        if stack.save_ra {
                            let ra_off = stack.arg_cons + stack.val + stack.alloc;
                            load_stack(&mut self.writer, ra_off, "ra");
                        }

                        if stack_size > RV_ADDI_LIMIT {
                            writeln!(self.writer, "  li t0, {}", stack_size).unwrap();
                            writeln!(self.writer, "  add sp, sp, t0").unwrap();
                        } else {
                            writeln!(self.writer, "  addi sp, sp, {}", stack_size).unwrap();
                        }
                        writeln!(self.writer, "  ret").unwrap();
                    }
                    ValueKind::Binary(b) => {
                        // load the operands
                        let lhs = b.lhs();
                        let rhs = b.rhs();
                        let lhs_off = stk_val.get(lhs, dfg);
                        let rhs_off = stk_val.get(rhs, dfg);
                        load_stack(&mut self.writer, lhs_off, "t0");
                        load_stack(&mut self.writer, rhs_off, "t1");

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
                        write_stack(&mut self.writer, pos, "t0");
                    }
                    ValueKind::Jump(j) => {
                        pass_bb_args!(j.target(), j.args());
                        writeln!(
                            self.writer,
                            "  j L{}",
                            &dfg.bb(j.target()).name().as_ref().unwrap()[1..]
                        )
                        .unwrap();
                    }
                    ValueKind::Branch(b) => {
                        let cond = b.cond();
                        let target_true = b.true_bb();
                        let target_false = b.false_bb();

                        let cond_off = stk_val.get(cond, dfg);
                        load_stack(&mut self.writer, cond_off, "t0");
                        // writeln!(
                        //     self.writer,
                        //     "  bnez t0, L{}",
                        //     &dfg.bb(target_true).name().as_ref().unwrap()[1..]
                        // )
                        // .unwrap();
                        // writeln!(
                        //     self.writer,
                        //     "  j L{}",
                        //     &dfg.bb(target_false).name().as_ref().unwrap()[1..]
                        // )
                        // .unwrap();

                        // Use long format to handle large functions.
                        let t_true = &dfg.bb(target_true).name().as_ref().unwrap()[1..];
                        let t_false = &dfg.bb(target_false).name().as_ref().unwrap()[1..];
                        writeln!(self.writer, "  bnez t0, B{}", self.long_branch_label).unwrap();
                        pass_bb_args!(target_false, b.false_args());
                        writeln!(self.writer, "  j L{}", t_false).unwrap();
                        pass_bb_args!(target_true, b.true_args());
                        writeln!(self.writer, "B{}: j L{}", self.long_branch_label, t_true)
                            .unwrap();
                        self.long_branch_label += 1;
                    }
                    ValueKind::Call(c) => {
                        for (i, arg) in c.args().iter().enumerate() {
                            let arg_off = stk_val.get(*arg, dfg);
                            if i < RV_N_ARGREG {
                                load_stack(&mut self.writer, arg_off, &format!("a{}", i));
                            } else {
                                load_stack(&mut self.writer, arg_off, "t0");
                                let stack_offset = (i - RV_N_ARGREG) * RV_WORD_SIZE;
                                write_stack(&mut self.writer, stack_offset, "t0");
                            }
                        }
                        let callee_data = prog.func(c.callee());
                        writeln!(self.writer, "  call {}", &callee_data.name()[1..]).unwrap();
                        if val.ty().is_i32() {
                            let val_off = stk_val.get(*val_handle, dfg);
                            write_stack(&mut self.writer, val_off, "a0");
                        } else if val.ty().is_unit() {
                            // no return value
                        } else {
                            panic!("unsupported return type {:?}", val.ty());
                        }
                    }
                    // GetElemPtr and GetPtr are essentially the same. They differ only in typing.
                    ValueKind::GetElemPtr(gep) => {
                        load_addr(
                            &mut self.writer,
                            gep.src(),
                            prog,
                            dfg,
                            "t0",
                            &mut stk_val,
                            &mut stk_var,
                        );
                        let idx_off = stk_val.get(gep.index(), dfg);
                        load_stack(&mut self.writer, idx_off, "t1");
                        let src_ty = get_type(prog, dfg, gep.src());
                        match src_ty.kind() {
                            TypeKind::Pointer(t) => {
                                let elem_size = if let TypeKind::Array(et, _n) = t.kind() {
                                    et.size()
                                } else {
                                    panic!("GEP on non-array pointer");
                                };
                                writeln!(&mut self.writer, "  li t2, {elem_size}").unwrap();
                                writeln!(&mut self.writer, "  mul t1, t1, t2").unwrap();
                                writeln!(&mut self.writer, "  add t0, t0, t1").unwrap();
                                let val_off = stk_val.get(*val_handle, dfg);
                                write_stack(&mut self.writer, val_off, "t0");
                            }
                            _ => panic!("GEP on non-pointer"),
                        }
                    }
                    ValueKind::GetPtr(gp) => {
                        load_addr(
                            &mut self.writer,
                            gp.src(),
                            prog,
                            dfg,
                            "t0",
                            &mut stk_val,
                            &mut stk_var,
                        );
                        let idx_off = stk_val.get(gp.index(), dfg);
                        load_stack(&mut self.writer, idx_off, "t1");
                        let src_ty = dfg.value(gp.src()).ty();
                        match src_ty.kind() {
                            TypeKind::Pointer(t) => {
                                let size = t.size();
                                writeln!(&mut self.writer, "  li t2, {}", size).unwrap();
                                writeln!(&mut self.writer, "  mul t1, t1, t2").unwrap();
                                writeln!(&mut self.writer, "  add t0, t0, t1").unwrap();
                                let val_off = stk_val.get(*val_handle, dfg);
                                write_stack(&mut self.writer, val_off, "t0");
                            }
                            _ => panic!("GEP on non-pointer"),
                        }
                    }
                    _ => unimplemented!("value kind {:?} not implemented", val.kind()),
                }
            }
        }

        writeln!(&mut self.writer).unwrap();
    }

    fn add_global_vars(&mut self, prog: &Program) {
        let globals = prog.inst_layout();

        writeln!(&mut self.writer, "  .data").unwrap();
        for v in globals {
            let val = prog.borrow_value(*v);
            match val.kind() {
                ValueKind::GlobalAlloc(ga) => {
                    let name = &val.name().as_ref().expect("global var must have a name")[1..];
                    let size = match val.ty().kind() {
                        TypeKind::Pointer(t) => t.size(),
                        _ => panic!("global alloc must be a pointer"),
                    };
                    // writeln!(&mut self.writer, "  .globl {name}\n{name}:\n  .zero {size}").unwrap();
                    // writeln!(&mut self.writer).unwrap();
                    writeln!(&mut self.writer, "  .globl {name}\n{name}:").unwrap();
                    let init = prog.borrow_value(ga.init());
                    match init.kind() {
                        ValueKind::ZeroInit(_) => {
                            // zero-initialized global variable
                            writeln!(&mut self.writer, "  .zero {}", size).unwrap();
                        }
                        ValueKind::Aggregate(_) => {
                            let vals = flatten_aggregate_const(prog, ga.init());
                            assert!(vals.len() * 4 == size, "aggregate size mismatch");
                            for v in vals {
                                writeln!(&mut self.writer, "  .word {}", v as u32).unwrap();
                            }
                        }
                        _ => panic!("invalid global variable initializer {:?}", init.kind()),
                    }
                }
                _ => panic!("global variable must be global_alloc"),
            }
        }
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
    koopa::ir::Type::set_ptr_size(4);
    let mut builder = SimpleRISCVBuilder::new(writer);
    builder.add_program(prog);
}
