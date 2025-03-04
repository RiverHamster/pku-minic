  .text
  .globl main
main:
  addi sp, sp, -8
  li t0, 1
  sw t0, 0(sp)
  li t0, 0
  sw t0, 4(sp)
L0:
  lw a0, 0(sp)
  addi sp, sp, 8
  ret
