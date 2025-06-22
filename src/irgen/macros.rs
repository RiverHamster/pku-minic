#[macro_export]
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

#[macro_export]
macro_rules! new_value {
    ($module:ident, $f_handle:expr) => {
        $module.prog.func_mut($f_handle).dfg_mut().new_value()
    };
}

#[macro_export]
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