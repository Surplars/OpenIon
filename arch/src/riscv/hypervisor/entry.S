/// VCpuContext layout offsets (#[repr(C)]):
///   x[0..31]  : offset 0   (32 * 8 = 256 bytes)
///   vsstatus  : offset 256
///   vsepc     : offset 264
///   vscause   : offset 272
///   vstval    : offset 280
///   vsatp     : offset 288
///   hstatus   : offset 296
///   hedeleg   : offset 304
///   hideleg   : offset 312
///   hgatp     : offset 320
///   id        : offset 328

.section .data
.global HOST_VCPU
.align 8
HOST_VCPU:
    .quad 0

.section .text

// ================================================================
// guest_run(vcpu: *mut VCpuContext) -> usize
// a0 = vCPU pointer. Returns scause on exit.
// ================================================================
.global guest_run
.type guest_run, @function
.balign 4
guest_run:
    // Save host callee-saved regs (ra + s0-s11 = 13 regs)
    addi sp, sp, -13*8
    sd   ra,  0*8(sp)
    sd   s0,  1*8(sp)
    sd   s1,  2*8(sp)
    sd   s2,  3*8(sp)
    sd   s3,  4*8(sp)
    sd   s4,  5*8(sp)
    sd   s5,  6*8(sp)
    sd   s6,  7*8(sp)
    sd   s7,  8*8(sp)
    sd   s8,  9*8(sp)
    sd   s9, 10*8(sp)
    sd   s10,11*8(sp)
    sd   s11,12*8(sp)

    // Store vCPU pointer in global
    la   t0, HOST_VCPU
    sd   a0, 0(t0)

    // Set hstatus: SPV=1 (enter VS-mode after sret)
    li   t0, (1 << 7)
    csrw hstatus, t0

    // Load guest vsepc -> sepc
    la   t0, HOST_VCPU
    ld   t0, 0(t0)
    ld   t1, 264(t0)     // vsepc
    csrw sepc, t1

    // Load guest GPRs x1-x31
    ld   x1,  1*8(t0)
    ld   x2,  2*8(t0)
    ld   x3,  3*8(t0)
    ld   x4,  4*8(t0)
    ld   x5,  5*8(t0)
    ld   x6,  6*8(t0)
    ld   x7,  7*8(t0)
    ld   x8,  8*8(t0)
    ld   x9,  9*8(t0)
    ld   x10, 10*8(t0)
    ld   x11, 11*8(t0)
    ld   x12, 12*8(t0)
    ld   x13, 13*8(t0)
    ld   x14, 14*8(t0)
    ld   x15, 15*8(t0)
    ld   x16, 16*8(t0)
    ld   x17, 17*8(t0)
    ld   x18, 18*8(t0)
    ld   x19, 19*8(t0)
    ld   x20, 20*8(t0)
    ld   x21, 21*8(t0)
    ld   x22, 22*8(t0)
    ld   x23, 23*8(t0)
    ld   x24, 24*8(t0)
    ld   x25, 25*8(t0)
    ld   x26, 26*8(t0)
    ld   x27, 27*8(t0)
    ld   x28, 28*8(t0)
    ld   x29, 29*8(t0)
    ld   x30, 30*8(t0)
    ld   x31, 31*8(t0)

    // Enter guest VS-mode
    sret


// ================================================================
// hypervisor_trap_vector - installed as stvec.
// Hardware jumps here on any trap from VS-mode guest.
// ================================================================
.global hypervisor_trap_vector
.type hypervisor_trap_vector, @function
.balign 4
hypervisor_trap_vector:
    // Load vCPU pointer from global
    la   s0, HOST_VCPU
    ld   s0, 0(s0)

    // Save ALL guest GPRs
    sd   x1,  1*8(s0)
    sd   x2,  2*8(s0)
    sd   x3,  3*8(s0)
    sd   x4,  4*8(s0)
    sd   x5,  5*8(s0)
    sd   x6,  6*8(s0)
    sd   x7,  7*8(s0)
    sd   x8,  8*8(s0)
    sd   x9,  9*8(s0)
    sd   x10, 10*8(s0)
    sd   x11, 11*8(s0)
    sd   x12, 12*8(s0)
    sd   x13, 13*8(s0)
    sd   x14, 14*8(s0)
    sd   x15, 15*8(s0)
    sd   x16, 16*8(s0)
    sd   x17, 17*8(s0)
    sd   x18, 18*8(s0)
    sd   x19, 19*8(s0)
    sd   x20, 20*8(s0)
    sd   x21, 21*8(s0)
    sd   x22, 22*8(s0)
    sd   x23, 23*8(s0)
    sd   x24, 24*8(s0)
    sd   x25, 25*8(s0)
    sd   x26, 26*8(s0)
    sd   x27, 27*8(s0)
    sd   x28, 28*8(s0)
    sd   x29, 29*8(s0)
    sd   x30, 30*8(s0)
    sd   x31, 31*8(s0)

    // Save guest VS-mode CSRs
    csrr t0, vsstatus
    sd   t0, 256(s0)
    csrr t0, vsepc
    sd   t0, 264(s0)
    csrr t0, vscause
    sd   t0, 272(s0)
    csrr t0, vstval
    sd   t0, 280(s0)

    // Call Rust handler: handle_vm_exit(vcpu, scause, stval)
    mv   a0, s0
    csrr a1, scause
    csrr a2, stval
    call handle_vm_exit

    // Return: 0 = resume guest, nonzero = exit to host
    beqz a0, .Lresume

    // --- Exit to host ---
    // a0 already has the exit reason
    // Restore host callee-saved regs
    ld   ra,  0*8(sp)
    ld   s0,  1*8(sp)
    ld   s1,  2*8(sp)
    ld   s2,  3*8(sp)
    ld   s3,  4*8(sp)
    ld   s4,  5*8(sp)
    ld   s5,  6*8(sp)
    ld   s6,  7*8(sp)
    ld   s7,  8*8(sp)
    ld   s8,  9*8(sp)
    ld   s9, 10*8(sp)
    ld   s10,11*8(sp)
    ld   s11,12*8(sp)
    addi sp, sp, 13*8
    ret

.Lresume:
    // --- Resume guest ---
    la   s0, HOST_VCPU
    ld   s0, 0(s0)

    // Restore guest VS-mode CSRs
    ld   t0, 256(s0)
    csrw vsstatus, t0
    ld   t0, 264(s0)
    csrw sepc, t0

    // Restore guest GPRs
    ld   x1,  1*8(s0)
    ld   x2,  2*8(s0)
    ld   x3,  3*8(s0)
    ld   x4,  4*8(s0)
    ld   x5,  5*8(s0)
    ld   x6,  6*8(s0)
    ld   x7,  7*8(s0)
    ld   x8,  8*8(s0)
    ld   x9,  9*8(s0)
    ld   x10, 10*8(s0)
    ld   x11, 11*8(s0)
    ld   x12, 12*8(s0)
    ld   x13, 13*8(s0)
    ld   x14, 14*8(s0)
    ld   x15, 15*8(s0)
    ld   x16, 16*8(s0)
    ld   x17, 17*8(s0)
    ld   x18, 18*8(s0)
    ld   x19, 19*8(s0)
    ld   x20, 20*8(s0)
    ld   x21, 21*8(s0)
    ld   x22, 22*8(s0)
    ld   x23, 23*8(s0)
    ld   x24, 24*8(s0)
    ld   x25, 25*8(s0)
    ld   x26, 26*8(s0)
    ld   x27, 27*8(s0)
    ld   x28, 28*8(s0)
    ld   x29, 29*8(s0)
    ld   x30, 30*8(s0)
    ld   x31, 31*8(s0)

    sret
