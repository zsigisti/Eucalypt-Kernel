.section .text
.global _start
_start:
    xor %rbp, %rbp
    call main
    movq %rax, %rdi
1:  jmp 1b
