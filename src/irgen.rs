use crate::sysy::ast::{self, Expr};
use core::panic;
use std::collections::HashMap;

use koopa::ir::{builder_traits::*, dfg::DataFlowGraph, *};

#[derive(Debug, Clone, Copy)]
enum SymbolTableEntry {
    Const(i32),
    Global(Value),
    Local(Value),
}

/// global states for processing the AST
pub struct IRBuilder {
    prog: Program,
    bb_idx: usize,
    // constants maps to integers, and variables maps to pointers
    syms: HashMap<String, SymbolTableEntry>,
}

impl IRBuilder {
    pub fn new() -> Self {
        Self {
            prog: Program::new(),
            bb_idx: 0,
            syms: HashMap::new(),
        }
    }

    fn eval_expr(&mut self, f_handle: Function, bb: BasicBlock, e: &ast::Expr) -> Value {
        // let dfg = self.prog.func(f_handle).dfg_mut();
        let zero = self
            .prog
            .func_mut(f_handle)
            .dfg_mut()
            .new_value()
            .integer(0);
        match e {
            Expr::LitInt(i) => self
                .prog
                .func_mut(f_handle)
                .dfg_mut()
                .new_value()
                .integer(i.0),
            Expr::UnaryExpr { op, expr } => {
                let expr_val = self.eval_expr(f_handle, bb, expr);
                let insn = match op {
                    ast::UnaryOp::Neg => self.prog.func_mut(f_handle).dfg_mut().new_value().binary(
                        BinaryOp::Sub,
                        zero,
                        expr_val,
                    ),
                    ast::UnaryOp::LNot => self
                        .prog
                        .func_mut(f_handle)
                        .dfg_mut()
                        .new_value()
                        .binary(BinaryOp::Eq, zero, expr_val),
                    ast::UnaryOp::Pos => expr_val,
                };

                // dummy operator does not generate instructions
                if *op != ast::UnaryOp::Pos {
                    self.prog
                        .func_mut(f_handle)
                        .layout_mut()
                        .bb_mut(bb)
                        .insts_mut()
                        .extend([insn]);
                }

                insn
            }
            Expr::BinaryExpr { op, lhs, rhs } => {
                use ast::BinaryOp::*;

                let ir_op = match op {
                    Add => BinaryOp::Add,
                    Sub => BinaryOp::Sub,
                    Mul => BinaryOp::Mul,
                    Div => BinaryOp::Div,
                    Mod => BinaryOp::Mod,
                    Lt => BinaryOp::Lt,
                    Gt => BinaryOp::Gt,
                    Leq => BinaryOp::Le,
                    Geq => BinaryOp::Ge,
                    Eq => BinaryOp::Eq,
                    Neq => BinaryOp::NotEq,
                    LAnd => BinaryOp::And,
                    LOr => BinaryOp::Or,
                    Index => unimplemented!(),
                };

                let mut lhs_val = self.eval_expr(f_handle, bb, lhs);
                let mut rhs_val = self.eval_expr(f_handle, bb, rhs);

                // TODO: short-circuiting
                if *op == LAnd || *op == LOr {
                    let lhs_logical = self.prog.func_mut(f_handle).dfg_mut().new_value().binary(
                        BinaryOp::NotEq,
                        zero,
                        lhs_val,
                    );
                    let rhs_logical = self.prog.func_mut(f_handle).dfg_mut().new_value().binary(
                        BinaryOp::NotEq,
                        zero,
                        rhs_val,
                    );
                    self.prog
                        .func_mut(f_handle)
                        .layout_mut()
                        .bb_mut(bb)
                        .insts_mut()
                        .extend([lhs_logical, rhs_logical]);

                    lhs_val = lhs_logical;
                    rhs_val = rhs_logical;
                }

                let insn = self
                    .prog
                    .func_mut(f_handle)
                    .dfg_mut()
                    .new_value()
                    .binary(ir_op, lhs_val, rhs_val);

                self.prog
                    .func_mut(f_handle)
                    .layout_mut()
                    .bb_mut(bb)
                    .insts_mut()
                    .extend([insn]);
                insn
            }
            Expr::Ident(ident) => {
                use SymbolTableEntry::*;
                match self.syms.get(&ident.0) {
                    Some(Const(i)) => self
                        .prog
                        .func_mut(f_handle)
                        .dfg_mut()
                        .new_value()
                        .integer(*i),
                    Some(_) => unimplemented!("ref non-const"),
                    // Some(Global(val)) => *val,
                    // Some(Local(val)) => *val,
                    None => panic!("undefined symbol: {}", ident.0),
                }
            }
        }
    }

    fn eval_i32_const(&self, e: &ast::Expr) -> i32 {
        use SymbolTableEntry::*;
        match e {
            Expr::LitInt(i) => i.0,
            Expr::Ident(i) => match self.syms.get(&i.0) {
                Some(Const(v)) => *v,
                Some(_) => panic!("symbol {} is not a constant", i.0),
                None => panic!("undefined symbol: {}", i.0),
            },
            Expr::UnaryExpr { op, expr } => {
                let val = self.eval_i32_const(expr);
                match op {
                    ast::UnaryOp::Neg => -val,
                    ast::UnaryOp::LNot => (val != 0) as i32,
                    ast::UnaryOp::Pos => val,
                }
            }
            Expr::BinaryExpr { op, lhs, rhs } => {
                let lhs = self.eval_i32_const(lhs);
                let rhs = self.eval_i32_const(rhs);
                match op {
                    ast::BinaryOp::Add => lhs + rhs,
                    ast::BinaryOp::Sub => lhs - rhs,
                    ast::BinaryOp::Mul => lhs * rhs,
                    ast::BinaryOp::Div => lhs / rhs,
                    ast::BinaryOp::Mod => lhs % rhs,
                    ast::BinaryOp::Lt => (lhs < rhs) as i32,
                    ast::BinaryOp::Gt => (lhs > rhs) as i32,
                    ast::BinaryOp::Leq => (lhs <= rhs) as i32,
                    ast::BinaryOp::Geq => (lhs >= rhs) as i32,
                    ast::BinaryOp::Eq => (lhs == rhs) as i32,
                    ast::BinaryOp::Neq => (lhs != rhs) as i32,
                    ast::BinaryOp::LAnd => (lhs != 0 && rhs != 0) as i32,
                    ast::BinaryOp::LOr => (lhs != 0 || rhs != 0) as i32,
                    ast::BinaryOp::Index => unimplemented!(),
                }
            }
        }
    }

    fn add_stmt(&mut self, f_handle: Function, bb: BasicBlock, b: &ast::Stmt) {
        match b {
            ast::Stmt::Return(ret) => {
                let ret_eval = ret.as_ref().map(|e| self.eval_expr(f_handle, bb, e));
                let ret = self
                    .prog
                    .func_mut(f_handle)
                    .dfg_mut()
                    .new_value()
                    .ret(ret_eval);
                self.prog
                    .func_mut(f_handle)
                    .layout_mut()
                    .bb_mut(bb)
                    .insts_mut()
                    .extend([ret]);
            }
            // TODO: other stmts
            _ => unimplemented!(),
        }
    }

    fn add_block(&mut self, f_handle: Function, b: &ast::Block) {
        // let f_data = self.prog.func_mut(f_handle);
        let bb = self
            .prog
            .func_mut(f_handle)
            .dfg_mut()
            .new_bb()
            .basic_block(Some(String::from("%") + &self.bb_idx.to_string()));
        self.prog
            .func_mut(f_handle)
            .layout_mut()
            .bbs_mut()
            .extend([bb]);
        self.bb_idx += 1;
        let ast::Block(items) = b;

        // save the possibly replaced symbols
        let sym_backup = items
            .iter()
            .filter_map(|item| match item {
                ast::BlockItem::Decl(d) => {
                    let vars = match d {
                        ast::Decl::Var(v) => &v.vars,
                        ast::Decl::Const(v) => &v.vars,
                    };
                    Some(
                        vars.iter()
                            .map(|v| (&v.name.0, self.syms.get(&v.name.0).copied())),
                    )
                }
                _ => None,
            })
            .flatten()
            .collect::<Vec<_>>();

        for item in items {
            match item {
                ast::BlockItem::Stmt(stmt) => {
                    self.add_stmt(f_handle, bb, stmt);
                    if let ast::Stmt::Return(_) = stmt {
                        break;
                    }
                }
                ast::BlockItem::Decl(decl) => match decl {
                    ast::Decl::Const(d) => match d {
                        ast::VarDecl {
                            base_ty: ast::BaseType::Int,
                            vars,
                        } => {
                            for v in vars {
                                let val = match &v.init {
                                    Some(ast::InitExpr::Scalar(e)) => {
                                        self.eval_i32_const(e)
                                    }
                                    Some(ast::InitExpr::Array(_)) => unimplemented!(),
                                    None => panic!("const {} uninitialized", v.name.0),
                                };
                                self.syms
                                    .insert(v.name.0.clone(), SymbolTableEntry::Const(val));
                            }
                        }
                        _ => panic!("base_ty of Decl must be int"),
                    },
                    ast::Decl::Var(d) => unimplemented!("VarDecl"),
                },
            }
        }

        for (name, val) in sym_backup {
            if let Some(val) = val {
                self.syms.get_mut(name).map(|v| *v = val);
            } else {
                self.syms.remove(name);
            }
        }
    }

    fn add_func(&mut self, f: &ast::FuncDef) {
        // TODO: function arguments
        let f_handle = self.prog.new_func(FunctionData::with_param_names(
            String::from("@") + &f.name.0,
            vec![],
            match f.ret_ty {
                ast::BaseType::Int => Type::get_i32(),
                ast::BaseType::Void => Type::get_unit(),
            },
        ));

        self.add_block(f_handle, &f.body);
    }

    pub fn parse(mut self, ast: &ast::TransUnit) -> Program {
        for item in ast.0.iter() {
            match item {
                ast::TransUnitItem::FuncDef(f) => self.add_func(&f),
                ast::TransUnitItem::Decl(d) => unimplemented!(),
            }
        }
        self.prog
    }
}

pub fn gen_ir(ast: &ast::TransUnit) -> Program {
    IRBuilder::new().parse(&ast)
}
