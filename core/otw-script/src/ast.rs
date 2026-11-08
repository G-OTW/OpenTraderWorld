//! Syntax tree. Control structures (`if`, `switch`, `for`, `while`) are expressions: in Pine they
//! return their last value; as statements they are wrapped in `StmtKind::Expr`.

use crate::diag::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Script {
    pub version: u8,
    pub body: Vec<Stmt>,
}

/// `float`, `array<float>`, `map<string, int>`, `chart.point`.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeRef {
    pub name: String,
    pub args: Vec<TypeRef>,
}

impl TypeRef {
    pub fn render(&self) -> String {
        if self.args.is_empty() {
            self.name.clone()
        } else {
            let a: Vec<String> = self.args.iter().map(|t| t.render()).collect();
            format!("{}<{}>", self.name, a.join(","))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Le => "<=",
            BinOp::Ge => ">=",
            BinOp::And => "and",
            BinOp::Or => "or",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Plus,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    pub name: Option<String>,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Num(f64),
    Int(i64),
    Str(String),
    Bool(bool),
    /// RRGGBBAA
    Color(u32),
    Na,
    /// A dotted path: `close`, `ta.tr`, `syminfo.mintick`, `obj.field`.
    Name(Vec<String>),
    /// A call on a dotted path: builtin, user function, or method on a variable (checker decides).
    Call { callee: Vec<String>, generic: Option<TypeRef>, args: Vec<Arg> },
    /// A call on a non-name receiver: `f(1).get(0)`.
    MethodCall { recv: Box<Expr>, name: String, args: Vec<Arg> },
    /// A field of a non-name receiver: `f().x`.
    Field { recv: Box<Expr>, name: String },
    /// History reference `base[offset]`.
    Index { base: Box<Expr>, offset: Box<Expr> },
    Unary { op: UnOp, e: Box<Expr> },
    Binary { op: BinOp, l: Box<Expr>, r: Box<Expr> },
    Ternary { c: Box<Expr>, a: Box<Expr>, b: Box<Expr> },
    Tuple(Vec<Expr>),
    If(Box<IfExpr>),
    Switch(Box<SwitchExpr>),
    For(Box<ForExpr>),
    ForIn(Box<ForInExpr>),
    While(Box<WhileExpr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IfExpr {
    pub cond: Expr,
    pub then: Block,
    pub elifs: Vec<(Expr, Block)>,
    pub els: Option<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchExpr {
    pub key: Option<Expr>,
    /// `None` pattern = the default arm (`=> …`).
    pub arms: Vec<(Option<Expr>, Block)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForExpr {
    pub var: String,
    pub from: Expr,
    pub to: Expr,
    pub by: Option<Expr>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForInExpr {
    pub idx: Option<String>,
    pub item: String,
    pub iter: Expr,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WhileExpr {
    pub cond: Expr,
    pub body: Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclMode {
    Plain,
    Var,
    Varip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Reassign,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Option<TypeRef>,
    pub default: Option<Expr>,
}

/// A field of a user-defined type: `float price = 0.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDef {
    pub ty: TypeRef,
    pub name: String,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Decl { mode: DeclMode, ty: Option<TypeRef>, name: String, value: Expr },
    TupleDecl { names: Vec<String>, value: Expr },
    /// `target` is a dotted path (`x`, `obj.field`).
    Assign { target: Vec<String>, op: AssignOp, value: Expr },
    Expr(Expr),
    Break,
    Continue,
    FuncDef { name: String, params: Vec<Param>, body: Block, export: bool },
    MethodDef { name: String, params: Vec<Param>, body: Block, export: bool },
    TypeDef { name: String, fields: Vec<FieldDef>, export: bool },
    EnumDef { name: String, export: bool },
    Import { path: String, alias: Option<String> },
}

fn args_sexpr(args: &[Arg]) -> String {
    args.iter()
        .map(|a| match &a.name {
            Some(n) => format!(" {n}={}", a.value.to_sexpr()),
            None => format!(" {}", a.value.to_sexpr()),
        })
        .collect()
}

impl Expr {
    /// Compact rendering used by tests: `(op a b)`, `(call f a k=v)`, `(idx x 1)`.
    pub fn to_sexpr(&self) -> String {
        match &self.kind {
            ExprKind::Num(v) => format!("{v}"),
            ExprKind::Int(v) => format!("{v}"),
            ExprKind::Str(s) => format!("{s:?}"),
            ExprKind::Bool(b) => format!("{b}"),
            ExprKind::Color(c) => format!("#{c:08X}"),
            ExprKind::Na => "na".into(),
            ExprKind::Name(p) => p.join("."),
            ExprKind::Call { callee, generic, args } => {
                let g = generic.as_ref().map(|t| format!("<{}>", t.args.iter().map(|a| a.render()).collect::<Vec<_>>().join(","))).unwrap_or_default();
                format!("(call {}{g}{})", callee.join("."), args_sexpr(args))
            }
            ExprKind::MethodCall { recv, name, args } => format!("(mcall {} {name}{})", recv.to_sexpr(), args_sexpr(args)),
            ExprKind::Field { recv, name } => format!("(field {} {name})", recv.to_sexpr()),
            ExprKind::Index { base, offset } => format!("(idx {} {})", base.to_sexpr(), offset.to_sexpr()),
            ExprKind::Unary { op, e } => {
                let o = match op {
                    UnOp::Neg => "neg",
                    UnOp::Plus => "pos",
                    UnOp::Not => "not",
                };
                format!("({o} {})", e.to_sexpr())
            }
            ExprKind::Binary { op, l, r } => format!("({} {} {})", op.symbol(), l.to_sexpr(), r.to_sexpr()),
            ExprKind::Ternary { c, a, b } => format!("(? {} {} {})", c.to_sexpr(), a.to_sexpr(), b.to_sexpr()),
            ExprKind::Tuple(items) => format!("[{}]", items.iter().map(|e| e.to_sexpr()).collect::<Vec<_>>().join(" ")),
            ExprKind::If(_) => "(if …)".into(),
            ExprKind::Switch(_) => "(switch …)".into(),
            ExprKind::For(_) => "(for …)".into(),
            ExprKind::ForIn(_) => "(for-in …)".into(),
            ExprKind::While(_) => "(while …)".into(),
        }
    }
}
