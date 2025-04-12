mod cli;
mod codegen;
mod irgen;
mod sysy;
use koopa;
use std::env;
use std::fs;
use std::io::Write;

fn main() {
    let conf = cli::parse_args(env::args());
    if conf.inputs.len() != 1 {
        unimplemented!("multiple translation unit");
    }

    let source = fs::read_to_string(&conf.inputs[0]).unwrap();
    let mut output_file = fs::File::create(&conf.output).unwrap();
    let parser = sysy::parser::TransUnitParser::new();
    let ast = parser.parse(&source).unwrap();

    if conf.output_type == cli::OutputType::AST {
        write!(output_file, "{:?}", ast).unwrap();
        return;
    }

    let ir_program = irgen::gen_ir(&ast);

    match conf.output_type {
        cli::OutputType::AST => {
            // AST is already handled above
        }
        cli::OutputType::Koopa => {
            let mut koopa_gen = koopa::back::Generator::with_visitor(
                output_file,
                koopa::back::koopa::Visitor::default(),
            );
            koopa_gen
                .generate_on(&ir_program)
                .expect("Failed to dump Koopa IR");
        }

        cli::OutputType::LLVM => {
            let mut llvm_gen = koopa::back::LlvmGenerator::with_visitor(
                output_file,
                koopa::back::llvm::Visitor::default(),
            );
            llvm_gen
                .generate_on(&ir_program)
                .expect("Failed to dump LLVM IR");
        }

        cli::OutputType::RISCV => {
            codegen::gen_riscv_simple(&ir_program, output_file);
        }
        cli::OutputType::Unknown => {
            unreachable!();
        }
    }
}
