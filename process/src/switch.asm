global context_switch

context_switch:
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov [rsi], rsp
    mov rsp, rdi

    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret