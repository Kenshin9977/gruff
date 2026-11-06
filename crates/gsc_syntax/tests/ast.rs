//! The typed AST views over small hand-written snippets.

use gsc_syntax::ast::{self, AstNode, Expr, Item, Stmt};
use gsc_syntax::{parse, Dialect};

fn file(text: &str) -> ast::SourceFile {
    let p = parse(text, &Dialect::superset());
    assert!(p.errors().is_empty(), "{:?}", p.errors());
    p.tree()
}

fn first_fn(text: &str) -> ast::FunctionDef {
    file(text).functions().next().expect("a function")
}

fn body_exprs(f: &ast::FunctionDef) -> Vec<Expr> {
    f.body()
        .unwrap()
        .stmts()
        .filter_map(|s| match s {
            Stmt::ExprStmt(e) => e.expr(),
            _ => None,
        })
        .collect()
}

#[test]
fn function_header() {
    let f = first_fn("/@ doc @/\nfunction private autoexec go( a, b = 2, &c, ... ) {}");
    assert_eq!(f.name().unwrap().text(), "go");
    assert!(f.is_private() && f.is_autoexec());
    assert!(f.doc_comment().is_some());
    let params: Vec<_> = f.param_list().unwrap().any_params().collect();
    assert_eq!(params.len(), 4);
    let ast::AnyParam::Param(b) = &params[1] else {
        panic!("b is a named parameter")
    };
    assert_eq!(b.name().unwrap().text(), "b");
    assert!(b.default_value().unwrap().expr().is_some());
    assert!(matches!(params[3], ast::AnyParam::VarargParam(_)));
}

#[test]
fn calls_with_receivers_and_modifiers() {
    let f = first_fn("function f() { self thread ns::go( 1, 2 ); [[ p ]](); x = &ns::go; }");
    let exprs = body_exprs(&f);
    let Expr::CallExpr(call) = &exprs[0] else {
        panic!("a call")
    };
    assert_eq!(call.receiver().unwrap().syntax().text(), "self");
    assert_eq!(call.modifier().unwrap().text(), "thread");
    let Some(Expr::QualifiedRef(q)) = call.callee() else {
        panic!("a qualified callee")
    };
    assert_eq!(q.qualifier().unwrap().syntax().text(), "ns");
    assert_eq!(q.name_ref().unwrap().text(), "go");
    assert_eq!(call.arg_list().unwrap().exprs().count(), 2);

    let Expr::PointerCallExpr(pc) = &exprs[1] else {
        panic!("a pointer call")
    };
    assert!(pc.receiver().is_none());
    assert_eq!(pc.target().unwrap().syntax().text(), "p");

    let Expr::AssignExpr(assign) = &exprs[2] else {
        panic!("an assignment")
    };
    assert_eq!(assign.op().unwrap().text(), "=");
    assert!(matches!(assign.rhs(), Some(Expr::FunctionRef(_))));
}

#[test]
fn precedence_shapes() {
    let f = first_fn("function f() { x = a + b * c; }");
    let Expr::AssignExpr(assign) = &body_exprs(&f)[0] else {
        panic!()
    };
    let Some(Expr::BinExpr(add)) = assign.rhs() else {
        panic!()
    };
    assert_eq!(add.op().unwrap().text(), "+");
    assert!(matches!(add.rhs(), Some(Expr::BinExpr(_))));
    assert_eq!(add.lhs().unwrap().syntax().text(), "a");
}

#[test]
fn fields_and_method_calls() {
    let f = first_fn("function f() { a.b.size; [[ obj ]]->run( 1 ); }");
    let exprs = body_exprs(&f);
    let Expr::FieldExpr(fe) = &exprs[0] else {
        panic!()
    };
    assert_eq!(fe.field().unwrap().text(), "size");
    assert_eq!(fe.base().unwrap().syntax().text(), "a.b");
    let Expr::MethodCallExpr(mc) = &exprs[1] else {
        panic!()
    };
    assert_eq!(mc.object().unwrap().syntax().text(), "obj");
    assert_eq!(mc.method().unwrap().text(), "run");
}

#[test]
fn items_and_classes() {
    let f = file("#using a\\b;\n#namespace demo;\nclass c : base { var m; constructor() {} }\n");
    assert_eq!(f.namespace().unwrap().text(), "demo");
    let items: Vec<Item> = f.items().collect();
    assert!(matches!(items[0], Item::UsingDirective(_)));
    let Item::ClassDef(c) = &items[2] else {
        panic!()
    };
    assert_eq!(c.name().unwrap().text(), "c");
    assert_eq!(c.class_base().unwrap().name_refs().count(), 1);
    assert_eq!(c.class_members().count(), 2);
}

#[test]
fn foreach_and_if_chain() {
    let f = first_fn(
        "function f() { foreach ( k, v in arr ) {} if ( a ) {} else if ( b ) {} else {} }",
    );
    let stmts: Vec<Stmt> = f.body().unwrap().stmts().collect();
    let Stmt::ForeachStmt(fe) = &stmts[0] else {
        panic!()
    };
    assert_eq!(fe.foreach_bindings().unwrap().names().count(), 2);
    assert_eq!(fe.iterable().unwrap().syntax().text(), "arr");
    let Stmt::IfStmt(i) = &stmts[1] else { panic!() };
    let Some(Stmt::IfStmt(inner)) = i.else_branch().unwrap().stmt() else {
        panic!("else if nests an IfStmt")
    };
    assert!(inner.else_branch().is_some());
}

#[test]
fn receivers_of_bracket_calls_and_first_significant_token() {
    let f = first_fn("function f() { self [[ p ]](); e thread [[ obj ]]->run(); }");
    let exprs = body_exprs(&f);
    let Expr::PointerCallExpr(pc) = &exprs[0] else {
        panic!("a pointer call")
    };
    assert_eq!(pc.receiver().unwrap().syntax().text(), "self");
    let Expr::MethodCallExpr(mc) = &exprs[1] else {
        panic!("a method call")
    };
    assert_eq!(mc.receiver().unwrap().syntax().text(), "e");
    assert_eq!(mc.modifier().unwrap().text(), "thread");
    // The call node holds no token of its own before `e`; the descendant lookup finds it.
    let first = gsc_syntax::first_significant_token(mc.syntax()).unwrap();
    assert_eq!(first.text(), "e");
}
