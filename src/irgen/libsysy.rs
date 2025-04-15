use koopa::ir::{FunctionData, Type};

use super::IRBuilder;

pub fn decl_sysy_stdlib(p: &mut IRBuilder) {
    let f = p.prog.new_func(FunctionData::new_decl(
        "@getint".into(),
        vec![],
        Type::get_i32(),
    ));
    p.funcs.insert("getint".into(), f);
    let f = p.prog.new_func(FunctionData::new_decl(
        "@getch".into(),
        vec![],
        Type::get_i32(),
    ));
    p.funcs.insert("getch".into(), f);
    let f = p.prog.new_func(FunctionData::new_decl(
        "@getarray".into(),
        vec![Type::get_pointer(Type::get_i32())],
        Type::get_i32(),
    ));
    p.funcs.insert("getarray".into(), f);
    let f = p.prog.new_func(FunctionData::new_decl(
        "@putint".into(),
        vec![Type::get_i32()],
        Type::get_unit(),
    ));
    p.funcs.insert("putint".into(), f);
    let f = p.prog.new_func(FunctionData::new_decl(
        "@putch".into(),
        vec![Type::get_i32()],
        Type::get_unit(),
    ));
    p.funcs.insert("putch".into(), f);
    let f = p.prog.new_func(FunctionData::new_decl(
        "@putarray".into(),
        vec![Type::get_i32(), Type::get_pointer(Type::get_i32())],
        Type::get_unit(),
    ));
    p.funcs.insert("putarray".into(), f);
    let f = p.prog.new_func(FunctionData::new_decl(
        "@starttime".into(),
        vec![],
        Type::get_unit(),
    ));
    p.funcs.insert("starttime".into(), f);
    let f = p.prog.new_func(FunctionData::new_decl(
        "@stoptime".into(),
        vec![],
        Type::get_unit(),
    ));
    p.funcs.insert("stoptime".into(), f);
}
