use core::panic;
use koopa::ir::{dfg::DataFlowGraph, *};
use std::{collections::HashMap, io};

const RV_ADDI_LIMIT: usize = 2047;
const RV_OFFSET_LIMIT: usize = 2047;
/// RV32
const RV_WORD_SIZE: usize = 4;
const RV_N_ARGREG: usize = 8;

struct SimpleRISCVBuilder<W: io::Write> {
    writer: W,
}

#[derive(Debug)]
struct StackSize {
    alloc: usize,
    val: usize,
    arg_cons: usize,
    save_regs: usize,
    save_ra: bool,
}

/// calculate the (stack size, size of values) of a function
fn stack_size(prog: &Program, f: Function) -> StackSize {
    let mut s: StackSize = StackSize {
        alloc: 0,
        val: 0,
        arg_cons: 0,
        save_regs: 0,
        save_ra: false,
    };
    for (_, v) in prog.func(f).dfg().values() {
        s.val += v.ty().size();
        eprintln!("Value type {:?} size {}", v.ty(), v.ty().size());
        match v.kind() {
            ValueKind::Alloc(_) => match v.ty().kind() {
                TypeKind::Pointer(t) => {
                    s.alloc += t.size();
                }
                _ => panic!("alloc value must be a pointer"),
            },
            ValueKind::Call(c) => {
                s.arg_cons = s.arg_cons.max(RV_WORD_SIZE * c.args().len());
                // Save RA.
                s.save_regs = RV_WORD_SIZE;
                s.save_ra = true;
            }
            _ => {}
        }
    }
    s
}

fn align(size: usize, align: usize) -> usize {
    (size + align - 1) / align * align
}

struct StackManager {
    offsets: HashMap<Value, usize>,
    bound: usize,
    deref: bool,
}

impl StackManager {
    fn new(base: usize, deref: bool) -> Self {
        Self {
            offsets: HashMap::new(),
            bound: base,
            deref,
        }
    }

    fn get(&mut self, v: Value, dfg: &DataFlowGraph) -> usize {
        // increment the bound to allocate, or return the value
        self.offsets.get(&v).copied().unwrap_or_else(|| {
            let pos = self.bound;
            let ty = dfg.value(v).ty();
            let size = if self.deref {
                match ty.kind() {
                    TypeKind::Pointer(t) => t.size(),
                    _ => panic!("Deref stack manager must manage pointers"),
                }
            } else {
                ty.size()
            };
            // eprintln!("allocated value type {:?} size {}, bound {}", dfg.value(v).ty(), dfg.value(v).ty().size(), self.bound);
            self.bound += size;
            self.offsets.insert(v, pos);
            pos
        })
    }
}

fn load_stack(writer: &mut impl io::Write, stack_offset: usize, reg: &str) {
    if stack_offset > RV_OFFSET_LIMIT {
        writeln!(writer, "  li {reg}, {stack_offset}").unwrap();
        writeln!(writer, "  add {reg}, sp, {reg}").unwrap();
        writeln!(writer, "  lw {reg}, 0({reg})").unwrap();
    } else {
        writeln!(writer, "  lw {reg}, {stack_offset}(sp)").unwrap();
    }
}

/// WARNING: overwrites t2
fn write_stack(writer: &mut impl io::Write, stack_offset: usize, reg: &str) {
    if stack_offset > RV_OFFSET_LIMIT {
        writeln!(writer, "  li t2, {}", stack_offset).unwrap();
        writeln!(writer, "  add t2, sp, t2").unwrap();
        writeln!(writer, "  sw {reg}, 0(t2)").unwrap();
    } else {
        writeln!(writer, "  sw {reg}, {stack_offset}(sp)").unwrap();
    }
}

impl<W: io::Write> SimpleRISCVBuilder<W> {
    fn new(writer: W) -> Self {
        Self { writer }
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
        eprintln!(
            "add_func {}, stack {:?}, allocation {}",
            f.name(),
            stack,
            stack_size
        );

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

        for (val, val_data) in dfg.values() {
            if let ValueKind::Integer(i) = val_data.kind() {
                let pos = stk_val.get(*val, dfg);
                writeln!(self.writer, "  li t0, {}", i.value()).unwrap();
                write_stack(&mut self.writer, pos, "t0");
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
                        if l.src().is_global() {
                            // Global: load by name
                            let val = prog.borrow_value(l.src());
                            let name =
                                &val.name().as_ref().expect("global var must have a name")[1..];
                            let val_off = stk_val.get(*val_handle, dfg);
                            writeln!(&mut self.writer, "  la t0, {name}").unwrap();
                            writeln!(&mut self.writer, "  lw t0, 0(t0)").unwrap();
                            write_stack(&mut self.writer, val_off, "t0");
                            // writeln!(&mut self.writer, "  sw t0, {}(sp)", val_off).unwrap();
                        } else {
                            let src_off = stk_var.get(l.src(), dfg);
                            let pos = stk_val.get(*val_handle, dfg);
                            load_stack(&mut self.writer, src_off, "t0");
                            write_stack(&mut self.writer, pos, "t0");
                        }
                    }
                    // only Store can access function arguments, guaranteed by the IR generator
                    ValueKind::Store(s) => {
                        if s.dest().is_global() {
                            let src_off = stk_val.get(s.value(), dfg);
                            load_stack(&mut self.writer, src_off, "t0");
                            // Global: store by name
                            let val = prog.borrow_value(s.dest());
                            let name =
                                &val.name().as_ref().expect("global var must have a name")[1..];
                            // overwrites t2
                            writeln!(&mut self.writer, "  la t2, {}", name).unwrap();
                            writeln!(&mut self.writer, "  sw t0, 0(t2)").unwrap();
                        } else {
                            let dst_off = stk_var.get(s.dest(), dfg);
                            let idx = arg_idx.get(&s.value());
                            match idx {
                                // argument
                                Some(i) => {
                                    if *i < RV_N_ARGREG {
                                        write_stack(&mut self.writer, dst_off, &format!("a{}", i));
                                    } else {
                                        let src_off = RV_WORD_SIZE * (*i - RV_N_ARGREG) + stack_size;
                                        load_stack(&mut self.writer, src_off, "t0");
                                        write_stack(&mut self.writer, dst_off, "t0");
                                    }
                                }
                                // local value (on stack)
                                None => {
                                    let src_off = stk_val.get(s.value(), dfg);
                                    load_stack(&mut self.writer, src_off, "t0");
                                    write_stack(&mut self.writer, dst_off, "t0");
                                }
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
                        let target = j.target();
                        writeln!(
                            self.writer,
                            "  j L{}",
                            &dfg.bb(target).name().as_ref().unwrap()[1..]
                        )
                        .unwrap();
                    }
                    ValueKind::Branch(b) => {
                        let cond = b.cond();
                        let target_true = b.true_bb();
                        let target_false = b.false_bb();

                        let cond_off = stk_val.get(cond, dfg);
                        load_stack(&mut self.writer, cond_off, "t0");
                        writeln!(
                            self.writer,
                            "  bnez t0, L{}",
                            &dfg.bb(target_true).name().as_ref().unwrap()[1..]
                        )
                        .unwrap();
                        writeln!(
                            self.writer,
                            "  j L{}",
                            &dfg.bb(target_false).name().as_ref().unwrap()[1..]
                        )
                        .unwrap();
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
            if val.kind().is_global_alloc() {
                let name = &val.name().as_ref().expect("global var must have a name")[1..];
                let size = match val.ty().kind() {
                    TypeKind::Pointer(t) => t.size(),
                    _ => panic!("global alloc must be a pointer"),
                };
                writeln!(&mut self.writer, "  .globl {name}\n{name}:\n  .zero {size}").unwrap();
                writeln!(&mut self.writer).unwrap();
            } else if val.kind().is_const() {
                panic!("global const must be a global alloc");
                // TODO: global arrays
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
    let mut builder = SimpleRISCVBuilder::new(writer);
    builder.add_program(prog);
}
