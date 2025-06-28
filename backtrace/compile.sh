#!/bin/bash

set -x

source="$1"
binary="$2"

asm_file="$(mktemp).s"
obj_file="$(mktemp).o"
SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )

cargo run -- -riscv "$source" -o "$asm_file"
clang -c "$asm_file" -o "$obj_file" -target riscv32-unknown-linux-elf -march=rv32im -mabi=ilp32
ld.lld "$obj_file" "$SCRIPT_DIR/backtrace.o" -L/opt/lib/riscv32 -lsysy -o "$binary"
