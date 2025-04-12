#!/bin/bash

NTESTS=10

for i in {1..1000}; do
    echo Test case $i
    python genprog.py > random.c
    ../../target/debug/minic -koopa random.c -o random.koopa || break
    ./koopac random.koopa -o random.ll || break
    llc random.ll -o random.s || break
    gcc random.s -o random || break

    gcc random.c -o random-ref

    ./random
    ret1=$?
    ./random-ref
    ret2=$?
    if [ $ret1 -eq $ret2 ]; then
        echo "Success: The outputs are the same."
    else
        echo "Failure: The outputs differ ($ret1 vs $ret2)."
        break
    fi
done
