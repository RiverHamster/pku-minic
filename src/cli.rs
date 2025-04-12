#[derive(Debug, PartialEq, Eq)]
pub enum OutputType {
    Unknown,
    AST,
    Koopa,
    RISCV,
    LLVM,
}

#[derive(Debug)]
pub struct Config {
    pub output_type: OutputType,
    pub inputs: Vec<String>,
    pub output: String,
}

pub fn parse_args(mut args: impl Iterator<Item = String>) -> Config {
    // skip program name
    args.next();

    let mut conf = Config {
        output_type: OutputType::Unknown,
        inputs: Vec::new(),
        output: String::new(),
    };

    let mut output_specified = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" => {
                assert!(!output_specified, "Multiple output files specified");
                conf.output = args.next().expect("No output file specified");
                output_specified = true;
            }
            "-ast" => {
                assert_eq!(
                    conf.output_type,
                    OutputType::Unknown,
                    "Duplicate output type"
                );
                conf.output_type = OutputType::AST;
            }
            "-koopa" => {
                assert_eq!(
                    conf.output_type,
                    OutputType::Unknown,
                    "Duplicate output type"
                );
                conf.output_type = OutputType::Koopa;
            }
            "-riscv" => {
                assert_eq!(
                    conf.output_type,
                    OutputType::Unknown,
                    "Duplicate output type"
                );
                conf.output_type = OutputType::RISCV;
            }
            "-llvm" => {
                assert_eq!(
                    conf.output_type,
                    OutputType::Unknown,
                    "Duplicate output type"
                );
                conf.output_type = OutputType::LLVM;
            }
            _ => {
                conf.inputs.push(arg);
            }
        }
    }

    assert!(output_specified, "No output file specified");

    assert_ne!(
        conf.output_type,
        OutputType::Unknown,
        "No output type specified"
    );

    conf
}
