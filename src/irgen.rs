use crate::{
    irgen::util::koopa_array_dims,
    sysy::ast::{self, Expr, VarDef},
};
use core::panic;
use std::collections::HashMap;

use koopa::ir::{builder_traits::*, *};

pub mod libsysy;
pub mod lvalue;
pub mod util;
#[macro_use]
pub(self) mod macros;

#[derive(Debug, Clone, Copy)]
pub(self) enum SymbolTableEntry {
    Const(i32),
    /// Alloc Value, which stores the variable.
    /// Const arrays are stored like variables.
    Var {
        v: Value,
        is_const: bool,
    },
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

    pub fn get_type(&self, v: &Value, f_handle: Function) -> Type {
        if v.is_global() {
            self.prog.borrow_value(*v).ty().clone()
        } else {
            self.prog.func(f_handle).dfg().value(*v).ty().clone()
        }
    }

    /// Add instructions to evaluate `e` in basic block `bb`.
    /// Returns the value of `e` and the converging basic block. Basic blocks
    /// may diverge due to short-circuit evaluation.
    #[must_use]
    fn eval_expr(
        &mut self,
        f_handle: Function,
        bb: BasicBlock,
        e: &ast::Expr,
    ) -> (Value, BasicBlock) {
        // eprintln!("eval_expr {:?}", e);
        // let dfg = self.prog.func(f_handle).dfg_mut();
        let zero = new_value!(self, f_handle).integer(0);
        // TODO: type_check?
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
                    // Get the array lvalue first. Then convert to rvalue.
                    Index => {
                        let (lval, bb1) = self.eval_lvalue(f_handle, bb, e);
                        let load = new_value!(self, f_handle).load(lval.ptr);
                        add_insn!(self, f_handle, bb1, [load]);
                        return (load, bb1);
                    }
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
                    Some(Var {
                        v: val,
                        is_const: _,
                    }) => {
                        let loaded = new_value!(self, f_handle).load(*val);
                        add_insn!(self, f_handle, bb, [loaded]);
                        (loaded, bb)
                    }
                    None => panic!("undefined symbol: {}", ident.0),
                }
            }
            Expr::FuncCall { name, args } => {
                let mut bb = bb;
                // let vals: Vec<_> = args
                //     .iter()
                //     .map(|e| {
                //         let (val, new_bb) = self.eval_expr(f_handle, bb, e);
                //         bb = new_bb;
                //         val
                //     })
                //     .collect();
                // TODO: Array parameter is not rvalue. Evaluate with eval_lvalue.

                let callee = self
                    .funcs
                    .get(&name.0)
                    .unwrap_or_else(|| panic!("undefined function: {}", name.0))
                    .clone();

                let callee_kind = self.prog.func(callee).ty().kind().clone();
                let args_type = match callee_kind {
                    TypeKind::Function(types, _) => types,
                    _ => unreachable!(),
                };
                let vals: Vec<_> = args
                    .iter()
                    .zip(args_type.iter())
                    .map(|(e, ty)| {
                        if ty.is_i32() {
                            let (val, new_bb) = self.eval_expr(f_handle, bb, e);
                            bb = new_bb;
                            val
                        } else {
                            let (lval, new_bb) = self.eval_lvalue(f_handle, bb, e);
                            bb = new_bb;
                            let lval_ty = self.get_type(&lval.ptr, f_handle);
                            // lval local array: *[i32; N] => *i32
                            // lval array param: **i32 => *i32
                            let deref_ty = match lval_ty.kind() {
                                TypeKind::Pointer(ty) => ty,
                                _ => panic!("lvalue must be a pointer"),
                            };
                            let arg = match deref_ty.kind() {
                                TypeKind::Array(_, _) => {
                                    // Convert sized array into a pointer.
                                    new_value!(self, f_handle).get_elem_ptr(lval.ptr, zero)
                                },
                                TypeKind::Pointer(_) => {
                                    new_value!(self, f_handle).load(lval.ptr)
                                }
                                _ => panic!("array parameter must be a pointer or array"),
                            };
                            add_insn!(self, f_handle, bb, [arg]);
                            arg
                        }
                    })
                    .collect();

                let call = new_value!(self, f_handle).call(callee, vals);

                add_insn!(self, f_handle, bb, [call]);
                (call, bb)
            }
        }
    }
    pub fn into_aggregate<'a>(
        &mut self,
        ty: &Type,
        vals: &mut impl Iterator<Item = &'a Value>,
        make_aggregate: &mut impl FnMut(&mut IRBuilder, Vec<Value>) -> Value,
    ) -> Value {
        match ty.kind() {
            TypeKind::Int32 => vals.next().unwrap().clone(),
            TypeKind::Array(elem_ty, size) => {
                let mut children = Vec::with_capacity(*size);
                for _ in 0..*size {
                    children.push(self.into_aggregate(elem_ty, vals, make_aggregate));
                }
                make_aggregate(self, children)
            }
            _ => panic!("into_aggregate should produce array types"),
        }
    }

    #[must_use]
    fn eval_array_init_impl(
        &mut self,
        mut env: Option<(Function, BasicBlock)>,
        ie: &ast::InitExpr,
        ty: &Type,
        zero: Value,
        is_const: bool,
    ) -> (Vec<Value>, Option<BasicBlock>) {
        // eprintln!("eval_array_init_impl ty {:?} ie {:?}", ty, ie);
        match ty.kind() {
            TypeKind::Int32 => {
                if let ast::InitExpr::Scalar(e) = ie {
                    // let (v, bb) = if let Some((f_handle, bb)) = env {
                    //     let (v, bb) = self.eval_expr(f_handle, bb, e);
                    //     (v, Some(bb))
                    // } else {
                    //     let evaluated = self.eval_i32_const(e);
                    //     (self.prog.new_value().integer(evaluated), env.map(|(_, bb)| bb))
                    // };
                    let (v, bb) = if is_const {
                        let evaluated = self.eval_i32_const(e);
                        if let Some((f_handle, bb)) = &env {
                            (
                                new_value!(self, *f_handle).integer(evaluated),
                                Some(bb.clone()),
                            )
                        } else {
                            (self.prog.new_value().integer(evaluated), None)
                        }
                    } else {
                        let (v, bb) = env.unwrap();
                        let (v, new_bb) = self.eval_expr(v, bb, e);
                        (v, Some(new_bb))
                    };
                    (vec![v], bb)
                } else {
                    panic!("int not initialized with scalar");
                }
            }
            TypeKind::Array(elem_ty, size) => {
                // let dims = koopa_array_dims(&ty);
                let elem_dims = koopa_array_dims(&elem_ty);
                let total_size = size * elem_dims.iter().product::<usize>();
                let ies = {
                    if let ast::InitExpr::Array(ies) = ie {
                        ies
                    } else {
                        panic!("array not initialized with aggregate");
                    }
                };

                let mut elems = vec![];
                for elem in ies {
                    match elem {
                        ast::InitExpr::Scalar(e) => {
                            let v = if let Some((f_handle, bb)) = &mut env {
                                let (v, new_bb) = self.eval_expr(*f_handle, *bb, e);
                                *bb = new_bb;
                                v
                            } else {
                                let evaluated = self.eval_i32_const(e);
                                let v = self.prog.new_value().integer(evaluated);
                                v
                            };
                            elems.push(v);
                        }
                        ast::InitExpr::Array(_) => {
                            let mut prod = 1_usize;
                            let mut agg_type = Type::get_i32();
                            for dim in elem_dims.iter().rev() {
                                prod *= dim;
                                if elems.len() % prod == 0 {
                                    agg_type = Type::get_array(agg_type, *dim);
                                } else {
                                    break;
                                }
                            }
                            let (mut v, new_bb) =
                                self.eval_array_init_impl(env, elem, &agg_type, zero, is_const);
                            if let Some(new_bb) = new_bb {
                                if let Some((_, bb)) = &mut env {
                                    *bb = new_bb;
                                } else {
                                    unreachable!();
                                }
                            }
                            elems.append(&mut v);
                        }
                    }
                }

                assert!(elems.len() <= total_size);

                while elems.len() < total_size {
                    elems.push(zero);
                }

                (elems, env.map(|(_, bb)| bb))
            }
            _ => {
                panic!("{:?} unsupported in initializers", ty);
            }
        }
    }

    /// Translate an AST InitExpr into an aggregate value.
    /// Evaluate to a variable if env is provided, const otherwise.
    fn eval_array_init(
        &mut self,
        env: Option<(Function, BasicBlock)>,
        ie: &ast::InitExpr,
        ty: &Type,
        make_aggregate: &mut impl FnMut(&mut IRBuilder, Vec<Value>) -> Value,
        zero: Value,
        is_const: bool,
    ) -> (Value, Option<BasicBlock>) {
        // eprintln!("eval_array_init ty {:?} ie {:?}", ty, ie);
        let (vals, bb) = self.eval_array_init_impl(env, ie, ty, zero, is_const);
        let v = self.into_aggregate(ty, &mut vals.iter(), make_aggregate);
        return (v, bb);
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
            ast::Stmt::Assign(lhs, rhs) => {
                // Refactor: general lvalue
                let (lval, bb1) = self.eval_lvalue(f_handle, bb, lhs);
                assert!(!lval.is_const, "Assignment to const lvalue");
                let (rval, bb2) = self.eval_expr(f_handle, bb1, rhs);

                // TODO: type check?
                let store = new_value!(self, f_handle).store(rval, lval.ptr);
                add_insn!(self, f_handle, bb2, [store]);
                vec![bb2]
            }
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
                ast::BlockItem::Decl(decl) => {
                    #[must_use]
                    fn add_array(
                        base_ty: ast::BaseType,
                        d: &VarDef,
                        is_const: bool,
                        irb: &mut IRBuilder,
                        f_handle: Function,
                        bb: BasicBlock,
                    ) -> BasicBlock {
                        let base_ty: Type = match base_ty {
                            ast::BaseType::Int => Type::get_i32(),
                            ast::BaseType::Void => panic!("void variable"),
                        };
                        assert!(!d.shape.is_empty(), "add_array on scalar");
                        let ty = d.shape.iter().rev().fold(base_ty, |ty, dim_expr| {
                            Type::get_array(ty, irb.eval_i32_const(dim_expr) as usize)
                        });
                        let alloc = new_value!(irb, f_handle).alloc(ty.clone());
                        add_insn!(irb, f_handle, bb, [alloc]);
                        irb.syms.insert(
                            d.name.0.clone(),
                            SymbolTableEntry::Var { v: alloc, is_const },
                        );
                        if let Some(ie) = &d.init {
                            let zero = new_value!(irb, f_handle).integer(0);
                            let (agg, new_bb) = irb.eval_array_init(
                                Some((f_handle, bb)),
                                &ie,
                                &ty,
                                &mut |irb, vs| new_value!(irb, f_handle).aggregate(vs),
                                zero,
                                is_const,
                            );
                            // Constant does not yield bb
                            let new_bb = new_bb.unwrap_or(bb);
                            let store = new_value!(irb, f_handle).store(agg, alloc);
                            add_insn!(irb, f_handle, new_bb, [store]);
                            new_bb
                        } else {
                            assert!(!is_const, "const array uninitialized");
                            bb
                        }
                    }
                    match decl {
                        ast::Decl::Const(d) => match d {
                            ast::VarDecl {
                                base_ty: ast::BaseType::Int,
                                vars,
                            } => {
                                for v in vars {
                                    let val = match &v.init {
                                        Some(ast::InitExpr::Scalar(e)) => self.eval_i32_const(e),
                                        Some(ast::InitExpr::Array(_)) => {
                                            bb = add_array(
                                                ast::BaseType::Int,
                                                v,
                                                true,
                                                self,
                                                f_handle,
                                                bb,
                                            );
                                            continue;
                                        }
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
                                if !v.shape.is_empty() {
                                    bb = add_array(d.base_ty, v, false, self, f_handle, bb);
                                } else {
                                    let ty = match d.base_ty {
                                        ast::BaseType::Int => Type::get_i32(),
                                        ast::BaseType::Void => panic!("void variable"),
                                    };
                                    let alloc = new_value!(self, f_handle).alloc(ty);
                                    add_insn!(self, f_handle, bb, [alloc]);
                                    self.syms.insert(
                                        v.name.0.clone(),
                                        SymbolTableEntry::Var {
                                            v: alloc,
                                            is_const: false,
                                        },
                                    );
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
                                            ast::InitExpr::Array(_e) => {
                                                panic!("array initializer in scalar");
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
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
                        // Type::get_i32()
                        if p.dims.is_empty() {
                            Type::get_i32()
                        } else {
                            assert!(
                                p.dims[0].is_none(),
                                "First dimension of array parameter must be empty"
                            );
                            let mut ty = Type::get_i32();
                            for dim in p.dims[1..].iter().rev() {
                                let dim = dim.as_ref().unwrap();
                                let dim = self.eval_i32_const(dim);
                                ty = Type::get_array(ty, dim as usize);
                            }
                            ty = Type::get_pointer(ty);
                            ty
                        }
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
            let arg_ty = self.get_type(&arg_val, f_handle);
            let alloc = new_value!(self, f_handle).alloc(arg_ty);
            let store = new_value!(self, f_handle).store(arg_val.clone(), alloc);
            add_insn!(self, f_handle, init_bb, [alloc, store]);
            self.syms.insert(
                f.params[i].name.0.clone(),
                SymbolTableEntry::Var {
                    v: alloc,
                    is_const: false,
                },
            );
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
        fn add_array(irb: &mut IRBuilder, v: &ast::VarDef) -> Value {
            let ty = v.shape.iter().rev().fold(Type::get_i32(), |ty, dim_expr| {
                Type::get_array(ty, irb.eval_i32_const(dim_expr) as usize)
            });
            if let Some(init) = &v.init {
                let zero = irb.prog.new_value().integer(0);
                let (agg, _none_bb) = irb.eval_array_init(
                    None,
                    init,
                    &ty,
                    &mut |irb, vs| irb.prog.new_value().aggregate(vs),
                    zero,
                    true,
                );
                assert!(_none_bb.is_none());
                irb.prog.new_value().global_alloc(agg)
            } else {
                let zeroinit = irb.prog.new_value().zero_init(ty.clone());
                irb.prog.new_value().global_alloc(zeroinit)
            }
        }
        fn add_sym(irb: &mut IRBuilder, name: &str, v: Value, is_const: bool) {
            irb.prog.set_value_name(v, Some(String::from("@") + name));
            irb.syms
                .insert(name.into(), SymbolTableEntry::Var { v, is_const });
        }
        match d {
            ast::Decl::Const(d) => match d {
                ast::VarDecl {
                    base_ty: ast::BaseType::Int,
                    vars,
                } => {
                    for v in vars {
                        let val = match &v.init {
                            Some(ast::InitExpr::Scalar(e)) => self.eval_i32_const(e),
                            Some(ast::InitExpr::Array(_)) => {
                                let alloc = add_array(self, v);
                                add_sym(self, &v.name.0, alloc, true);
                                continue;
                            }
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
                        if !v.shape.is_empty() {
                            let alloc = add_array(self, v);
                            add_sym(self, &v.name.0, alloc, false);
                        } else {
                            let zeros = self.prog.new_value().zero_init(Type::get_i32());
                            let alloc = self.prog.new_value().global_alloc(zeros);
                            add_sym(self, &v.name.0, alloc, false);

                            if let Some(init) = v.init.as_ref() {
                                let (init_global, mut bb) = self.init_global.clone().unwrap();
                                match init {
                                    ast::InitExpr::Scalar(e) => {
                                        let (val, new_bb) = self.eval_expr(init_global, bb, &e);
                                        let store = new_value!(self, init_global).store(val, alloc);
                                        add_insn!(self, init_global, new_bb, [store]);
                                        bb = new_bb;
                                    }
                                    ast::InitExpr::Array(_v) => {
                                        panic!("array initializer in scalar")
                                    }
                                }
                                self.init_global = Some((init_global, bb));
                            }
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
