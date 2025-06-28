#!/bin/bash
# Accepts a decimal address, report the name of the function.

binary=$1
addr=$2
hex_addr="0x$(printf "%X" $addr)"
addr2line --exe "$binary" -f $hex_addr | head -n 1
