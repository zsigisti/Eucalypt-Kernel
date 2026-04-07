global jump_usermode

jump_usermode:
    mov rcx, 0xc0000082
    wrmsr
    mov rcx, 0xc0000080
    rdmsr
    or eax, 1
    wrmsr
    mov rcx, 0xc0000081
    rdmsr
    mov edx, 0x00180008
    wrmsr

    mov rcx, rdi
    mov r11, 0x202
    mov rsp, 0x00007FFFFFFF0000
    o64 sysret