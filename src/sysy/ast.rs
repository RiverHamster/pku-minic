#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum UnaryOp {
    Pos,
    Neg,
    LNot,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Lt,
    Gt,
    Leq,
    Geq,
    Eq,
    Neq,
    LAnd,
    LOr,
    Index,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Ident(pub String);

impl Into<Expr> for Ident {
    fn into(self) -> Expr {
        Expr::Ident(self)
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct LitInt(pub i32);

impl Into<Expr> for LitInt {
    fn into(self) -> Expr {
        Expr::LitInt(self)
    }
}

#[derive(Debug, Clone)]
pub enum Expr {
    Ident(Ident),
    LitInt(LitInt),
    FuncCall {
        name: Ident,
        args: Vec<Expr>,
    },
    UnaryExpr {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    BinaryExpr {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseType {
    Int,
    Void,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Assign(Expr, Expr),
    Expr(Expr),
    Return(Option<Expr>),
    Block(Box<Block>),
    If(Expr, Box<Stmt>, Box<Stmt>),
    While(Expr, Box<Stmt>),
    Break,
    Continue,
    Empty,
}

#[derive(Debug, Clone)]
pub enum InitExpr {
    Scalar(Expr),
    Array(Vec<InitExpr>),
}

#[derive(Debug, Clone)]
pub struct VarDef {
    pub name: Ident,
    pub shape: Vec<Expr>,
    pub init: Option<InitExpr>,
}

#[derive(Debug, Clone)]
pub struct VarDecl {
    pub base_ty: BaseType,
    pub vars: Vec<VarDef>,
}

#[derive(Debug, Clone)]
pub enum Decl {
    Var(VarDecl),
    Const(VarDecl),
}

impl Into<BlockItem> for Stmt {
    fn into(self) -> BlockItem {
        BlockItem::Stmt(self)
    }
}

#[derive(Debug, Clone)]
pub enum BlockItem {
    Stmt(Stmt),
    Decl(Decl),
}

#[derive(Debug, Clone)]
pub struct Block(pub Vec<BlockItem>);

#[derive(Debug, Clone)]
pub struct FuncParam {
    pub ty: BaseType,
    pub name: Ident,
    pub dims: Vec<Option<Expr>>,
}

#[derive(Debug, Clone)]
pub struct FuncDef {
    pub ret_ty: BaseType,
    pub name: Ident,
    pub params: Vec<FuncParam>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub enum TransUnitItem {
    FuncDef(FuncDef),
    Decl(Decl),
}

#[derive(Debug, Clone)]
pub struct TransUnit(pub Vec<TransUnitItem>);
