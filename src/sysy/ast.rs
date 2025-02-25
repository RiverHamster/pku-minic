#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Pos,
    Neg,
    LNot,
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Debug)]
pub enum Expr {
    Ident(Ident),
    LitInt(LitInt),
    UnaryExpr(UnaryExpr),
    BinaryExpr(BinaryExpr),
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct BinaryExpr {
    pub op: BinaryOp,
    pub lhs: Box<Expr>,
    pub rhs: Box<Expr>,
}

impl Into<Expr> for BinaryExpr {
    fn into(self) -> Expr {
        Expr::BinaryExpr(self)
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct UnaryExpr {
    pub op: UnaryOp,
    pub expr: Box<Expr>,
}

impl Into<Expr> for UnaryExpr {
    fn into(self) -> Expr {
        Expr::UnaryExpr(self)
    }
}

pub struct KeyInt();
