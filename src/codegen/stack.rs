use std::{collections::HashMap, io};

use crate::codegen::{util::get_value_kind, RV_ADDI_LIMIT};

use super::{RV_OFFSET_LIMIT, RV_WORD_SIZE};
use koopa::ir::{dfg::DataFlowGraph, *};

#[derive(Debug)]
pub struct StackSize {
    pub alloc: usize,
    pub val: usize,
    pub arg_cons: usize,
    pub save_regs: usize,
    pub save_ra: bool,
}

/// calculate the (stack size, size of values) of a function
pub fn stack_size(prog: &Program, f: Function) -> StackSize {
    let mut s: StackSize = StackSize {
        alloc: 0,
        val: 0,
        arg_cons: 0,
        save_regs: 0,
        save_ra: false,
    };
    for (_, v) in prog.func(f).dfg().values() {
        s.val += v.ty().size();
        // eprintln!("Value type {:?} size {}", v.ty(), v.ty().size());
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

pub struct StackManager {
    pub offsets: HashMap<Value, usize>,
    pub bound: usize,
    pub deref: bool,
}

impl StackManager {
    pub fn new(base: usize, deref: bool) -> Self {
        Self {
            offsets: HashMap::new(),
            bound: base,
            deref,
        }
    }

    pub fn get(&mut self, v: Value, dfg: &DataFlowGraph) -> usize {
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

pub fn load_stack(writer: &mut impl io::Write, stack_offset: usize, reg: &str) {
    if stack_offset > RV_OFFSET_LIMIT {
        writeln!(writer, "  li {reg}, {stack_offset}").unwrap();
        writeln!(writer, "  add {reg}, sp, {reg}").unwrap();
        writeln!(writer, "  lw {reg}, 0({reg})").unwrap();
    } else {
        writeln!(writer, "  lw {reg}, {stack_offset}(sp)").unwrap();
    }
}

/// WARNING: overwrites t2
pub fn write_stack(writer: &mut impl io::Write, stack_offset: usize, reg: &str) {
    if stack_offset > RV_OFFSET_LIMIT {
        writeln!(writer, "  li t2, {}", stack_offset).unwrap();
        writeln!(writer, "  add t2, sp, t2").unwrap();
        writeln!(writer, "  sw {reg}, 0(t2)").unwrap();
    } else {
        writeln!(writer, "  sw {reg}, {stack_offset}(sp)").unwrap();
    }
}

/// Load the address of an alloc value (or GEP pointer) into a register.
/// Handles both local and global allocs. Used for implementing GEP and GP.
pub fn load_addr(
    writer: &mut impl io::Write,
    v: Value,
    prog: &Program,
    dfg: &DataFlowGraph,
    reg: &str,
    stk_val: &mut StackManager,
    stk_var: &mut StackManager,
) {
    if v.is_global() {
        let val = prog.borrow_value(v);
        assert!(matches!(val.kind(), ValueKind::GlobalAlloc(_)));
        let name = &val.name().as_ref().expect("global var must have a name")[1..];
        // Load address of global symbol.
        writeln!(writer, "  la {reg}, {name}").unwrap();
    } else {
        let val = dfg.value(v);
        match val.kind() {
            ValueKind::Alloc(_) => {
                // Address of local alloc can be directly obtained, not stored in the Alloc value.
                // This is a inconsistent design choice, to save some instructions to load values.
                let offset = stk_var.get(v, dfg);
                if offset < RV_ADDI_LIMIT {
                    writeln!(writer, "  addi {reg}, sp, {offset}").unwrap();
                } else {
                    // Use li to load large offsets.
                    writeln!(writer, "  li {reg}, {offset}").unwrap();
                    writeln!(writer, "  add {reg}, sp, {reg}").unwrap();
                }
            }
            ValueKind::GetElemPtr(_) | ValueKind::GetPtr(_) | ValueKind::Load(_) => {
                // Result of GEP and GP are stored in the value.
                // TODO: Load is added here to handle the case where a pointer is loaded from stack.
                let offset = stk_val.get(v, dfg);
                eprintln!("Load GEP/GP address {}(sp) into {reg}", offset);
                load_stack(writer, offset, reg);
            }
            _ => panic!("load_addr unsupported {:?}", val.kind()),
        };
    }
}

/// Load scalar value referenced by pointer `v` into register `reg`.
/// This sometimes use extra instructions, to simplify the implementation.
pub fn load_val(
    writer: &mut impl io::Write,
    v: Value,
    prog: &Program,
    dfg: &DataFlowGraph,
    reg: &str,
    stk_val: &mut StackManager,
    stk_var: &mut StackManager,
) {
    let v_kind = get_value_kind(prog, dfg, v);
    if let ValueKind::Integer(i) = v_kind {
        writeln!(writer, "  li {reg}, {}", i.value()).unwrap();
    } else {
        load_addr(writer, v, prog, dfg, reg, stk_val, stk_var);
        writeln!(writer, "  lw {reg}, 0({reg})").unwrap();
    }
}

/// Store the scalar in register `reg` into the memory location `v`.
/// This sometimes use extra instructions, to simplify the implementation.
/// Use register `addr_reg` to hold the address.
pub fn store_val(
    writer: &mut impl io::Write,
    dest: Value,
    prog: &Program,
    dfg: &DataFlowGraph,
    reg: &str,
    addr_reg: &str,
    stk_val: &mut StackManager,
    stk_var: &mut StackManager,
) {
    load_addr(writer, dest, prog, dfg, addr_reg, stk_val, stk_var);
    writeln!(writer, "  sw {reg}, 0({addr_reg})").unwrap();
}
