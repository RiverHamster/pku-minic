use koopa::ir::{Type, TypeKind};

pub fn koopa_array_dims(ty: &Type) -> Vec<usize> {
    match ty.kind() {
        TypeKind::Array(elem_ty, size) => {
            let mut rem = koopa_array_dims(elem_ty);
            rem.insert(0, *size);
            rem
        }
        _ => vec![],
    }
}
