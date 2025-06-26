use koopa::ir::Program;

pub trait IRPass {
    fn run(&mut self, prog: &mut Program);
}
