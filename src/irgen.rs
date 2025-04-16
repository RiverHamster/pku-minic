use crate::sysy::ast::{self, Expr};
use core::panic;
use std::collections::HashMap;

use koopa::ir::{builder_traits::*, *};

pub mod libsysy;

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
    /// Constants maps to integers, and variables maps to pointers
    syms: HashMap<String, SymbolTableEntry>,
    funcs: HashMap<String, Function>,
    /// The function to initialize global variables. Called before main.
    init_global: Option<(Function, BasicBlock)>,
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

#[derive(Clone, Copy)]
struct LoopEnv {
    /// The basic block checking the loop condition.
    bb_check: BasicBlock,
    /// The basic block exiting the loop
    bb_exit: BasicBlock,
}

impl IRBuilder {
    pub fn new() -> Self {
        Self {
            prog: Program::new(),
            bb_idx: 0,
            syms: HashMap::new(),
            funcs: HashMap::new(),
            init_global: None,
        }
    }

    /// Add instructions to evaluate `e` in basic block `bb`.
    /// Returns the value of `e` and the converging basic block. Basic blocks
    /// may diverge due to short-circuit evaluation.
    fn eval_expr(
        &mut self,
        f_handle: Function,
        bb: BasicBlock,
        e: &ast::Expr,
    ) -> (Value, BasicBlock) {
        // let dfg = self.prog.func(f_handle).dfg_mut();
        let zero = new_value!(self, f_handle).integer(0);
        match e {
            Expr::LitInt(i) => (new_value!(self, f_handle).integer(i.0), bb),
            Expr::UnaryExpr { op, expr } => {
                // Overwrite the current bb.
                let (expr_val, bb) = self.eval_expr(f_handle, bb, expr);
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

                (insn, bb)
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

                // Overwrite the current bb.
                let (lhs_val, bb) = self.eval_expr(f_handle, bb, lhs);

                if *op == LAnd || *op == LOr {
                    let expr_val = new_value!(self, f_handle).alloc(Type::get_i32());
                    // Short-circuit.
                    let bb_short = add_bb!(self, f_handle);
                    // Not short-circuit.
                    let bb_full = add_bb!(self, f_handle);
                    let bb_conv = add_bb!(self, f_handle);
                    let jmp_conv1 = new_value!(self, f_handle).jump(bb_conv);
                    let jmp_conv2 = new_value!(self, f_handle).jump(bb_conv);

                    // Branch on the LHS value.
                    let br1 = if *op == LAnd {
                        new_value!(self, f_handle).branch(lhs_val, bb_full, bb_short)
                    } else {
                        new_value!(self, f_handle).branch(lhs_val, bb_short, bb_full)
                    };
                    // BB: Allocation and branch.
                    add_insn!(self, f_handle, bb, [expr_val, br1]);

                    let short_val =
                        new_value!(self, f_handle).integer(if *op == LAnd { 0 } else { 1 });
                    let short_store = new_value!(self, f_handle).store(short_val, expr_val);

                    // BB_SHORT: Write the short-circuit result, then converge.
                    add_insn!(self, f_handle, bb_short, [short_store, jmp_conv1]);

                    // BB_FULL: Evaluate RHS, and write result
                    let (rhs_val, bb_full) = self.eval_expr(f_handle, bb_full, rhs);
                    let rhs_logical =
                        new_value!(self, f_handle).binary(BinaryOp::NotEq, zero, rhs_val);
                    let full_store = new_value!(self, f_handle).store(rhs_logical, expr_val);
                    add_insn!(
                        self,
                        f_handle,
                        bb_full,
                        [rhs_logical, full_store, jmp_conv2]
                    );

                    // BB_CONV: Read the value from stack(alloc).
                    let loaded_val = new_value!(self, f_handle).load(expr_val);
                    add_insn!(self, f_handle, bb_conv, [loaded_val]);
                    (loaded_val, bb_conv)
                } else {
                    // Overwrite the current bb.
                    let (rhs_val, bb) = self.eval_expr(f_handle, bb, rhs);

                    let insn = new_value!(self, f_handle).binary(ir_op, lhs_val, rhs_val);
                    add_insn!(self, f_handle, bb, [insn]);
                    (insn, bb)
                }
            }
            Expr::Ident(ident) => {
                use SymbolTableEntry::*;
                match self.syms.get(&ident.0) {
                    Some(Const(i)) => (new_value!(self, f_handle).integer(*i), bb),
                    // Some(Global(val)) => *val,
                    Some(Var(val)) => {
                        let loaded = new_value!(self, f_handle).load(*val);
                        add_insn!(self, f_handle, bb, [loaded]);
                        (loaded, bb)
                    }
                    None => panic!("undefined symbol: {}", ident.0),
                }
            }
            Expr::FuncCall { name, args } => {
                let mut bb = bb;
                let vals: Vec<_> = args
                    .iter()
                    .map(|e| {
                        let (val, new_bb) = self.eval_expr(f_handle, bb, e);
                        bb = new_bb;
                        val
                    })
                    .collect();

                let callee = self
                    .funcs
                    .get(&name.0)
                    .unwrap_or_else(|| panic!("undefined function: {}", name.0))
                    .clone();
                let call = new_value!(self, f_handle).call(callee, vals);

                add_insn!(self, f_handle, bb, [call]);
                (call, bb)
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
            Expr::FuncCall { name: _, args: _ } => panic!("function call in constant expression"),
        }
    }

    /// Append a statement to the current open basic block `bb`.
    /// Return the open basic blocks.
    #[must_use]
    fn add_stmt(
        &mut self,
        f_handle: Function,
        bb: BasicBlock,
        lenv: Option<LoopEnv>,
        s: &ast::Stmt,
    ) -> Vec<BasicBlock> {
        // eprintln!("add_stmt bb {:?} stmt {:?}", bb, s);
        match s {
            ast::Stmt::Return(ret) => {
                match ret {
                    None => {
                        let ret = new_value!(self, f_handle).ret(None);
                        add_insn!(self, f_handle, bb, [ret]);
                    }
                    Some(e) => {
                        let (val, bb) = self.eval_expr(f_handle, bb, e);
                        let ret = new_value!(self, f_handle).ret(Some(val));
                        add_insn!(self, f_handle, bb, [ret]);
                    }
                };
                vec![]
            }
            ast::Stmt::Assign(lhs, rhs) => match lhs {
                ast::Expr::Ident(ident) => {
                    let lval_entry = self
                        .syms
                        .get(&ident.0)
                        .expect(&format!("undefined symbol: {}", ident.0))
                        .clone();
                    let (rval, bb) = self.eval_expr(f_handle, bb, rhs).clone();
                    if let SymbolTableEntry::Var(var) = lval_entry {
                        let store = new_value!(self, f_handle).store(rval, var);
                        add_insn!(self, f_handle, bb, [store]);
                    } else {
                        panic!("assign to non-lvalue");
                    }

                    // eprintln!("returned bb {:?}", bb);
                    vec![bb]
                }
                ast::Expr::BinaryExpr {
                    op: ast::BinaryOp::Index,
                    lhs: _base,
                    rhs: _index,
                } => unimplemented!("array index assign"),
                _ => panic!("assign to non-lvalue"),
            },
            ast::Stmt::Block(b) => self.add_block(f_handle, Some(bb), lenv, b),
            ast::Stmt::Empty => vec![bb],
            ast::Stmt::Expr(e) => {
                // Expr may have side effects, e.g. function call
                let (_val, bb) = self.eval_expr(f_handle, bb, e);
                vec![bb]
            }
            ast::Stmt::If(cond, then_stmt, else_stmt) => {
                let (cond_val, bb) = self.eval_expr(f_handle, bb, cond);
                let then_bb = add_bb!(self, f_handle);
                let else_bb = add_bb!(self, f_handle);
                let then_open = self.add_stmt(f_handle, then_bb, lenv, then_stmt);
                let else_open = self.add_stmt(f_handle, else_bb, lenv, else_stmt);
                let branch = new_value!(self, f_handle).branch(cond_val, then_bb, else_bb);
                add_insn!(self, f_handle, bb, [branch]);

                then_open.into_iter().chain(else_open.into_iter()).collect()
            }
            ast::Stmt::While(cond, body) => {
                let bb_check = add_bb!(self, f_handle);
                let bb_body = add_bb!(self, f_handle);
                let bb_exit = add_bb!(self, f_handle);

                // Current basic block go to the check block
                let jmp = new_value!(self, f_handle).jump(bb_check);
                add_insn!(self, f_handle, bb, [jmp]);

                // Check the condition
                let (cond_val, bb_check_tail) = self.eval_expr(f_handle, bb_check, cond);
                let branch = new_value!(self, f_handle).branch(cond_val, bb_body, bb_exit);
                add_insn!(self, f_handle, bb_check_tail, [branch]);

                let open_bbs =
                    self.add_stmt(f_handle, bb_body, Some(LoopEnv { bb_check, bb_exit }), body);

                // Converge the basic blocks
                for open_bb in open_bbs {
                    let jmp = new_value!(self, f_handle).jump(bb_check);
                    add_insn!(self, f_handle, open_bb, [jmp]);
                }

                vec![bb_exit]
            }
            ast::Stmt::Break => {
                if let Some(lenv) = lenv {
                    let jmp = new_value!(self, f_handle).jump(lenv.bb_exit);
                    add_insn!(self, f_handle, bb, [jmp]);
                    vec![]
                } else {
                    panic!("break outside of loop");
                }
            }
            ast::Stmt::Continue => {
                if let Some(lenv) = lenv {
                    let jmp = new_value!(self, f_handle).jump(lenv.bb_check);
                    add_insn!(self, f_handle, bb, [jmp]);
                    vec![]
                } else {
                    panic!("continue outside of loop");
                }
            } // TODO: other stmts
              // _ => unimplemented!("statement {:?} unimplemented", s),
        }
    }

    #[must_use]
    fn add_block(
        &mut self,
        f_handle: Function,
        bb: Option<BasicBlock>,
        lenv: Option<LoopEnv>,
        b: &ast::Block,
    ) -> Vec<BasicBlock> {
        // eprintln!("add_block bb {:?} block {:?}", bb, b);
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
            // eprintln!("add_block item {:?}", item);
            match item {
                ast::BlockItem::Stmt(stmt) => {
                    let bbs = self.add_stmt(f_handle, bb, lenv, stmt);
                    // eprintln!("returned bbs {:?}", bbs);
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
                                ast::BaseType::Void => panic!("void variable"),
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
                                        let bbs = self.add_stmt(
                                            f_handle,
                                            bb,
                                            lenv,
                                            &ast::Stmt::Assign(
                                                ast::Expr::Ident(v.name.clone()),
                                                e.clone(),
                                            ),
                                        );
                                        assert!(bbs.len() == 1);
                                        bb = bbs[0];
                                    }
                                    ast::InitExpr::Array(_e) => unimplemented!("array init"),
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
        let f_handle = self.prog.new_func(FunctionData::with_param_names(
            String::from("@") + &f.name.0,
            // vec![],
            f.params
                .iter()
                .map(|p| {
                    (Some(String::from("@") + p.name.0.as_str()), {
                        // TODO: array parameters
                        assert!(p.ty == ast::BaseType::Int);
                        Type::get_i32()
                    })
                })
                .collect(),
            match f.ret_ty {
                ast::BaseType::Int => Type::get_i32(),
                ast::BaseType::Void => Type::get_unit(),
            },
        ));

        self.funcs.insert(f.name.0.clone(), f_handle);

        // Copy all arguments to stack, and add them to symbol table.
        let init_bb = add_bb!(self, f_handle);

        let syms_backup = f
            .params
            .iter()
            .map(|p| (p.name.0.clone(), self.syms.get(&p.name.0).copied()))
            .collect::<Vec<_>>();

        for i in 0..f.params.len() {
            let arg_val = self.prog.func(f_handle).params()[i];
            let alloc = new_value!(self, f_handle).alloc(Type::get_i32());
            let store = new_value!(self, f_handle).store(arg_val.clone(), alloc);
            add_insn!(self, f_handle, init_bb, [alloc, store]);
            self.syms
                .insert(f.params[i].name.0.clone(), SymbolTableEntry::Var(alloc));
        }

        if f.name.0 == "main" {
            let call = new_value!(self, f_handle).call(self.init_global.unwrap().0, vec![]);
            add_insn!(self, f_handle, init_bb, [call]);
        }

        let opens = self.add_block(f_handle, Some(init_bb), None, &f.body);
        for bb in opens {
            match f.ret_ty {
                ast::BaseType::Void => {
                    let ret = new_value!(self, f_handle).ret(None);
                    add_insn!(self, f_handle, bb, [ret]);
                }
                _ => {
                    let zero = new_value!(self, f_handle).integer(0);
                    let ret = new_value!(self, f_handle).ret(Some(zero));
                    add_insn!(self, f_handle, bb, [ret]);
                }
            }
        }

        for (name, val) in syms_backup {
            match val {
                Some(v) => {
                    self.syms.insert(name, v);
                }
                None => {
                    self.syms.remove(&name);
                }
            }
        }
    }

    fn add_global_decl(&mut self, d: &ast::Decl) {
        match d {
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
            ast::Decl::Var(d) => match d {
                ast::VarDecl {
                    base_ty: ast::BaseType::Int,
                    vars,
                } => {
                    for v in vars {
                        let base_ty = match d.base_ty {
                            ast::BaseType::Int => Type::get_i32(),
                            ast::BaseType::Void => panic!("void variable"),
                        };
                        let _ty = v.shape.iter().rev().fold(base_ty, |ty, dim_expr| {
                            Type::get_array(ty, self.eval_i32_const(dim_expr) as usize)
                        });
                        // TODO: global array
                        let zeros = self.prog.new_value().zero_init(Type::get_i32());
                        let alloc = self.prog.new_value().global_alloc(zeros);
                        self.syms
                            .insert(v.name.0.clone(), SymbolTableEntry::Var(alloc));

                        if let Some(init) = v.init.as_ref() {
                            let (init_global, mut bb) = self.init_global.clone().unwrap();
                            match init {
                                ast::InitExpr::Scalar(e) => {
                                    let (val, new_bb) = self.eval_expr(init_global, bb, &e);
                                    let store = new_value!(self, init_global).store(val, alloc);
                                    add_insn!(self, init_global, new_bb, [store]);
                                    bb = new_bb;
                                }
                                ast::InitExpr::Array(_v) => unimplemented!("array init"),
                            }
                            self.init_global = Some((init_global, bb));
                        }
                    }
                }
                _ => panic!("base_ty of Decl must be int"),
            },
        }
    }

    pub fn parse(mut self, ast: &ast::TransUnit) -> Program {
        // libsysy::decl_sysy_stdlib(&mut self.prog);
        libsysy::decl_sysy_stdlib(&mut self);

        let f_init_global = self.prog.new_func(FunctionData::new(
            "@Tfb77qtUahKLryrSePOqOmMEdL3rcXYICN6C9XFY".into(),
            vec![],
            Type::get_unit(),
        ));
        let init_bb = add_bb!(self, f_init_global);
        self.init_global = Some((f_init_global, init_bb));

        for item in ast.0.iter() {
            match item {
                ast::TransUnitItem::FuncDef(f) => self.add_func(&f),
                ast::TransUnitItem::Decl(d) => self.add_global_decl(d),
            }
        }

        let ret = new_value!(self, f_init_global).ret(None);
        add_insn!(self, f_init_global, self.init_global.unwrap().1, [ret]);

        self.prog
    }
}

pub fn gen_ir(ast: &ast::TransUnit) -> Program {
    IRBuilder::new().parse(&ast)
}
