#[derive(Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Pos,
    Neg,
    LNot,
}

#[derive(Debug, PartialEq, Eq)]
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

#[derive(Debug, PartialEq, Eq)]
pub struct Ident(pub String);

impl Into<Expr> for Ident {
    fn into(self) -> Expr {
        Expr::Ident(self)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct LitInt(pub i32);

impl Into<Expr> for LitInt {
    fn into(self) -> Expr {
        Expr::LitInt(self)
    }
}

#[derive(Debug)]
pub enum Expr {
    Ident(Ident),
    LitInt(LitInt),
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

#[derive(Debug)]
pub enum BaseType {
    Int,
    Void,
}

#[derive(Debug)]
pub enum Stmt {
    // TODO: Assignment
    Expr(Expr),
    Return(Option<Expr>),
    Block(Box<Block>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    Break,
    Continue,
}

#[derive(Debug)]
pub enum InitExpr {
    Scalar(Expr),
    Array(Vec<InitExpr>),
}

#[derive(Debug)]
pub struct VarDef {
    pub name: Ident,
    pub shape: Vec<Expr>,
    pub init: Option<InitExpr>,
}

#[derive(Debug)]
pub struct VarDecl {
    pub base_ty: BaseType,
    pub vars: Vec<VarDef>,
}

#[derive(Debug)]
pub enum Decl {
    Var(VarDecl),
    Const(VarDecl),
}

impl Into<BlockItem> for Stmt {
    fn into(self) -> BlockItem {
        BlockItem::Stmt(self)
    }
}

#[derive(Debug)]
pub enum BlockItem {
    Stmt(Stmt),
    Decl(Decl),
}

#[derive(Debug)]
pub struct Block(pub Vec<BlockItem>);

#[derive(Debug)]
pub struct FuncDef {
    pub ret_ty: BaseType,
    pub name: Ident,
    pub params: Vec<(BaseType, Ident)>,
    pub body: Block,
}

#[derive(Debug)]
pub enum TransUnitItem {
    FuncDef(FuncDef),
    Decl(Decl),
}

#[derive(Debug)]
pub struct TransUnit(pub Vec<TransUnitItem>);
