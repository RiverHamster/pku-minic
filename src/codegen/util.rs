use koopa::ir::{dfg::DataFlowGraph, Program, Type, Value, ValueKind};

pub fn align(size: usize, align: usize) -> usize {
    (size + align - 1) / align * align
}

pub fn flatten_aggregate_const(prog: &Program, vh: Value) -> Vec<i32> {
    let v = prog.borrow_value(vh);
    match v.kind() {
        ValueKind::Integer(i) => vec![i.value()],
        ValueKind::Aggregate(agg) => agg
            .elems()
            .iter()
            .map(|elem| flatten_aggregate_const(prog, *elem))
            .flatten()
            .collect(),
        _ => panic!("aggregate contains non-aggregate value"),
    }
}

pub fn flatten_local_aggregate(dfg: &DataFlowGraph, vh: Value) -> Vec<Value> {
    // Local aggregates have all values local.
    let v = dfg.value(vh);
    match v.kind() {
        ValueKind::Aggregate(agg) => agg
            .elems()
            .iter()
            .map(|elem| flatten_local_aggregate(dfg, *elem))
            .flatten()
            .collect(),
        _ => vec![vh],
    }
}

pub fn get_type(prog: &Program, dfg: &DataFlowGraph, v: Value) -> Type {
    if v.is_global() {
        prog.borrow_value(v).ty().clone()
    } else {
        dfg.value(v).ty().clone()
    }
}

pub fn get_value_kind(prog: &Program, dfg: &DataFlowGraph, v: Value) -> ValueKind {
    if v.is_global() {
        prog.borrow_value(v).kind().clone()
    } else {
        dfg.value(v).kind().clone()
    }
}
