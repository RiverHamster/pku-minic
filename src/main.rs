mod cli;
mod sysy;
use std::env;
use std::fs;

fn main() {
    let conf = cli::parse_args(env::args());
    if conf.inputs.len() != 1 {
        unimplemented!("multiple translation unit");
    }

    let source = fs::read_to_string(&conf.inputs[0]).unwrap();
    let ast = sysy::parser::ExprParser::new().parse(&source).unwrap();
    println!("{:?}", ast);
}
