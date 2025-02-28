mod cli;
mod sysy;
mod irgen;
mod codegen;
use std::env;
use std::fs;
use koopa;

fn main() {
    let conf = cli::parse_args(env::args());
    if conf.inputs.len() != 1 {
        unimplemented!("multiple translation unit");
    }

    let source = fs::read_to_string(&conf.inputs[0]).unwrap();
    let parser = sysy::parser::TransUnitParser::new();
    let ast = parser.parse(&source).unwrap();
    let ir_program = irgen::gen_ir(&ast);
    let output_file = fs::File::create(&conf.output).unwrap();
    if conf.output_type == cli::OutputType::Koopa {
        let mut koopa_gen = koopa::back::Generator::with_visitor(output_file, koopa::back::koopa::Visitor::default());
        koopa_gen.generate_on(&ir_program).expect("Failed to dump Koopa IR");
        return;
    }

    if conf.output_type == cli::OutputType::RISCV {
        codegen::gen_riscv_simple(&ir_program, output_file);
        return;
    }
}
