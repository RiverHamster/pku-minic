use koopa::ir::{builder::LocalInstBuilder, BasicBlock, Function, Value};

use crate::{add_insn, irgen::SymbolTableEntry, new_value, sysy::ast};

use super::IRBuilder;

#[derive(Debug, Clone, Copy)]
pub(super) struct LValueInfo {
    /// The pointer to the value.
    pub ptr: Value,
    /// Const arrays are lvalue but not assignable.
    pub is_const: bool,
}

impl IRBuilder {
    /// Evaluate an lvalue expression. Return the pointer to the value.
    /// Example: i32 -> *i32, [i32; N] -> *[i32; N]; *i32 -> **i32;
    pub(super) fn eval_lvalue(
        &mut self,
        f_handle: Function,
        mut bb: BasicBlock,
        e: &ast::Expr,
    ) -> (LValueInfo, BasicBlock) {
        // eprintln!("eval_lvalue: {:?}", e);
        let info = match e {
            ast::Expr::Ident(i) => {
                let entry = self.syms.get(&i.0).unwrap_or_else(|| {
                    panic!("symbol {} not found", i.0);
                });
                if let SymbolTableEntry::Var { v, is_const } = entry {
                    LValueInfo {
                        ptr: v.clone(),
                        is_const: *is_const,
                    }
                } else {
                    panic!("{} is not a variable", i.0);
                }
            }
            ast::Expr::BinaryExpr {
                op: ast::BinaryOp::Index,
                lhs,
                rhs,
            } => {
                let (lval, bb1) = self.eval_lvalue(f_handle, bb, lhs);
                bb = bb1;
                let (index, bb2) = self.eval_expr(f_handle, bb, rhs);
                bb = bb2;
                assert!(
                    self.get_type(&index, f_handle).is_i32(),
                    "array index must be int"
                );

                let lty = self.get_type(&lval.ptr, f_handle);
                let lkind_deref = match lty.kind() {
                    koopa::ir::TypeKind::Pointer(t) => t.kind(),
                    _ => unreachable!("lvalue must be a pointer"),
                };

                let indexed = match lkind_deref {
                    koopa::ir::TypeKind::Array(_, _) => {
                        new_value!(self, f_handle).get_elem_ptr(lval.ptr, index)
                    }
                    koopa::ir::TypeKind::Pointer(_) => {
                        let deref = new_value!(self, f_handle).load(lval.ptr);
                        add_insn!(self, f_handle, bb, [deref]);
                        new_value!(self, f_handle).get_ptr(deref, index)
                    }
                    _ => panic!("type {:?} cannot be indexed", lty),
                };
                add_insn!(self, f_handle, bb, [indexed]);

                LValueInfo {
                    ptr: indexed,
                    is_const: lval.is_const,
                }
            }
            _ => panic!("{:?} is not lvalue expr", e),
        };
        (info, bb)
    }
}
