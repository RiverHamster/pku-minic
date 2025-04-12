use crate::sysy::ast::{self, Expr};
use std::collections::HashMap;

use koopa::ir::{builder_traits::*, *};

#[derive(Debug, Clone, Copy)]
enum SymbolTableEntry {
    Const(i32),
    // Alloc Value, which stores the variable
    Var(Value),
}

/// global states for processing the AST
pub struct IRBuilder {
    prog: Program,
    bb_idx: usize,
    // constants maps to integers, and variables maps to pointers
    syms: HashMap<String, SymbolTableEntry>,
}

macro_rules! add_insn {
    ($module:ident, $f_handle:expr, $bb:expr, $insn:expr) => {
        $module
            .prog
            .func_mut($f_handle)
            .layout_mut()
            .bb_mut($bb)
            .insts_mut()
            .extend($insn);
    };
}

macro_rules! new_value {
    ($module:ident, $f_handle:expr) => {
        $module.prog.func_mut($f_handle).dfg_mut().new_value()
    };
}

macro_rules! add_bb {
    ($module:ident, $f_handle:expr) => {{
        let bb = $module
            .prog
            .func_mut($f_handle)
            .dfg_mut()
            .new_bb()
            .basic_block(Some(String::from("%") + &$module.bb_idx.to_string()));
        $module
            .prog
            .func_mut($f_handle)
            .layout_mut()
            .bbs_mut()
            .extend([bb]);
        $module.bb_idx += 1;
        bb
    }};
}

impl IRBuilder {
    pub fn new() -> Self {
        Self {
            prog: Program::new(),
            bb_idx: 0,
            syms: HashMap::new(),
        }
    }

    // TODO: type information
    fn eval_expr(&mut self, f_handle: Function, bb: BasicBlock, e: &ast::Expr) -> Value {
        // let dfg = self.prog.func(f_handle).dfg_mut();
        let zero = new_value!(self, f_handle).integer(0);
        match e {
            Expr::LitInt(i) => new_value!(self, f_handle).integer(i.0),
            Expr::UnaryExpr { op, expr } => {
                let expr_val = self.eval_expr(f_handle, bb, expr);
                let insn = match op {
                    ast::UnaryOp::Neg => {
                        new_value!(self, f_handle).binary(BinaryOp::Sub, zero, expr_val)
                    }
                    ast::UnaryOp::LNot => {
                        new_value!(self, f_handle).binary(BinaryOp::Eq, zero, expr_val)
                    }
                    ast::UnaryOp::Pos => expr_val,
                };

                // dummy operator does not generate instructions
                if *op != ast::UnaryOp::Pos {
                    add_insn!(self, f_handle, bb, [insn]);
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
                    let lhs_logical =
                        new_value!(self, f_handle).binary(BinaryOp::NotEq, zero, lhs_val);
                    let rhs_logical =
                        new_value!(self, f_handle).binary(BinaryOp::NotEq, zero, rhs_val);
                    add_insn!(self, f_handle, bb, [lhs_logical, rhs_logical]);

                    lhs_val = lhs_logical;
                    rhs_val = rhs_logical;
                }

                let insn = new_value!(self, f_handle).binary(ir_op, lhs_val, rhs_val);
                add_insn!(self, f_handle, bb, [insn]);
                insn
            }
            Expr::Ident(ident) => {
                use SymbolTableEntry::*;
                match self.syms.get(&ident.0) {
                    Some(Const(i)) => new_value!(self, f_handle).integer(*i),
                    // Some(Global(val)) => *val,
                    Some(Var(val)) => {
                        let loaded = new_value!(self, f_handle).load(*val);
                        add_insn!(self, f_handle, bb, [loaded]);
                        loaded
                    }
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
                    ast::UnaryOp::LNot => (val == 0) as i32,
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

    /// Append a statement to the current open basic block `bb`.
    /// Return the open basic blocks.
    fn add_stmt(&mut self, f_handle: Function, bb: BasicBlock, b: &ast::Stmt) -> Vec<BasicBlock> {
        match b {
            ast::Stmt::Return(ret) => {
                let ret_eval = ret.as_ref().map(|e| self.eval_expr(f_handle, bb, e));
                let ret = new_value!(self, f_handle).ret(ret_eval);
                add_insn!(self, f_handle, bb, [ret]);
                vec![]
            }
            ast::Stmt::Assign(lhs, rhs) => match lhs {
                ast::Expr::Ident(ident) => {
                    let lval_entry = self
                        .syms
                        .get(&ident.0)
                        .expect(&format!("undefined symbol: {}", ident.0))
                        .clone();
                    let rval = self.eval_expr(f_handle, bb, rhs).clone();
                    if let SymbolTableEntry::Var(var) = lval_entry {
                        let store = new_value!(self, f_handle).store(rval, var);
                        add_insn!(self, f_handle, bb, [store]);
                    } else {
                        panic!("assign to non-lvalue");
                    }
                    vec![bb]
                }
                ast::Expr::BinaryExpr {
                    op: ast::BinaryOp::Index,
                    lhs: base,
                    rhs: index,
                } => unimplemented!("array index assign"),
                _ => panic!("assign to non-lvalue"),
            },
            ast::Stmt::Block(b) => self.add_block(f_handle, Some(bb), b),
            ast::Stmt::Empty => vec![bb],
            // TODO: Expr may have side effects
            ast::Stmt::Expr(_) => {
                eprintln!("WARN: side effects in expr statement");
                vec![bb]
            }
            ast::Stmt::If(cond, then_stmt, else_stmt) => {
                let cond_val = self.eval_expr(f_handle, bb, cond);
                let then_bb = add_bb!(self, f_handle);
                let else_bb = add_bb!(self, f_handle);
                let then_open = self.add_stmt(f_handle, then_bb, then_stmt);
                let else_open = self.add_stmt(f_handle, else_bb, else_stmt);
                let branch = new_value!(self, f_handle).branch(cond_val, then_bb, else_bb);
                add_insn!(self, f_handle, bb, [branch]);

                then_open.into_iter().chain(else_open.into_iter()).collect()
            }
            // TODO: other stmts
            _ => unimplemented!("statement {:?} unimplemented", b),
        }
    }

    fn add_block(
        &mut self,
        f_handle: Function,
        bb: Option<BasicBlock>,
        b: &ast::Block,
    ) -> Vec<BasicBlock> {
        let mut bb = bb.unwrap_or_else(|| add_bb!(self, f_handle));
        let mut closed = false;
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
                    let bbs = self.add_stmt(f_handle, bb, stmt);
                    if bbs.is_empty() {
                        closed = true;
                        break;
                    }
                    bb = if bbs.len() > 1 {
                        // re-converge the divergent basic blocks
                        let new_bb = add_bb!(self, f_handle);
                        for open_bb in bbs {
                            let jump = new_value!(self, f_handle).jump(new_bb);
                            add_insn!(self, f_handle, open_bb, [jump]);
                        }
                        new_bb
                    } else {
                        bbs[0]
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
                                    Some(ast::InitExpr::Scalar(e)) => self.eval_i32_const(e),
                                    Some(ast::InitExpr::Array(_)) => unimplemented!(),
                                    None => panic!("const {} uninitialized", v.name.0),
                                };
                                self.syms
                                    .insert(v.name.0.clone(), SymbolTableEntry::Const(val));
                            }
                        }
                        _ => panic!("base_ty of Decl must be int"),
                    },
                    ast::Decl::Var(d) => {
                        for v in &d.vars {
                            let base_ty = match d.base_ty {
                                ast::BaseType::Int => Type::get_i32(),
                                ast::BaseType::Void => Type::get_unit(),
                            };
                            let ty = v.shape.iter().rev().fold(base_ty, |ty, dim_expr| {
                                Type::get_array(ty, self.eval_i32_const(dim_expr) as usize)
                            });
                            let alloc = new_value!(self, f_handle).alloc(ty);
                            add_insn!(self, f_handle, bb, [alloc]);
                            self.syms
                                .insert(v.name.0.clone(), SymbolTableEntry::Var(alloc));
                            if let Some(init) = &v.init {
                                match init {
                                    ast::InitExpr::Scalar(e) => {
                                        self.add_stmt(
                                            f_handle,
                                            bb,
                                            &ast::Stmt::Assign(
                                                ast::Expr::Ident(v.name.clone()),
                                                e.clone(),
                                            ),
                                        );
                                    }
                                    ast::InitExpr::Array(e) => unimplemented!("array init"),
                                }
                            }
                        }
                    }
                },
            }
        }

        // roll back to the last frame
        for (name, val) in sym_backup {
            if let Some(val) = val {
                self.syms.get_mut(name).map(|v| *v = val);
            } else {
                self.syms.remove(name);
            }
        }

        if closed {
            vec![]
        } else {
            vec![bb]
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

        self.add_block(f_handle, None, &f.body);
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
