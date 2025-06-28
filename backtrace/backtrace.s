.globl rv_backtrace
.type rv_backtrace, @function

rv_backtrace:
    addi sp, sp, -16
    sw ra, 12(sp)
    sw fp, 8(sp)
    addi fp, sp, 16

    mv a0, ra
    call putint
    li a0, 0x0A
    call putch
    # fp is saved
    # t0 is the fp of last frame
    lw t0, 8(sp)
loop0:
    beqz t0, end_rv_backtrace
    # last ra
    lw a0, -4(t0)
    sw t0, 4(sp)
    call putint
    li a0, 0x0A
    call putch
    lw t0, 4(sp)
    # move to last frame
    lw t0, -8(t0)
    # last ra
    j loop0
end_rv_backtrace:
    lw ra, 12(sp)
    lw fp, 8(sp)
    mv sp, fp
    ret

.size rv_backtrace, . - rv_backtrace