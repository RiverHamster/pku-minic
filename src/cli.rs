#[derive(Debug, PartialEq, Eq)]
pub enum OutputType {
    Unknown,
    Koopa,
    RISCV,
}

#[derive(Debug)]
pub struct Config {
    pub output_type: OutputType,
    pub inputs: Vec<String>,
    pub output: Vec<String>,
}

pub fn parse_args(mut args: impl Iterator<Item = String>) -> Config {
    // skip program name
    args.next();

    let mut conf = Config {
        output_type: OutputType::Unknown,
        inputs: Vec::new(),
        output: Vec::new(),
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" => {
                conf.output.push(args.next().unwrap());
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
            _ => {
                conf.inputs.push(arg);
            }
        }
    }

    assert_ne!(
        conf.output_type,
        OutputType::Unknown,
        "No output type specified"
    );

    conf
}
