  .text
  .globl main
main:
  addi sp, sp, -16
  li t0, 0
  sw t0, 0(sp)
  li t0, 0
  sw t0, 4(sp)
  li t0, 2
  sw t0, 8(sp)
L0:
  lw t0, 4(sp)
  lw t1, 8(sp)
  sub t0, t0, t1
  sw t0, 12(sp)
  lw a0, 12(sp)
  addi sp, sp, 16
  ret
