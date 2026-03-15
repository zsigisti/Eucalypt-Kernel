#include <sys/stat.h>
#include <sys/types.h>
#include <sys/fcntl.h>
#include <sys/times.h>
#include <sys/errno.h>
#include <sys/time.h>
#include <stdio.h>

static long syscall(long num, long a1, long a2, long a3) {
    long ret;
    __asm__ volatile (
        "syscall"
        : "=a"(ret)
        : "0"(num), "D"(a1), "S"(a2), "d"(a3)
        : "memory"
    );
    return ret;
}

void _exit(int status) {
    while (1) {}
}

int _close(int fd) {
    return syscall(4, fd, 0, 0);
}

int _fstat(int fd, struct stat *st) {
    st->st_mode = S_IFCHR;
    return 0;
}

int _isatty(int fd) {
    return (fd < 3) ? 1 : 0;
}

int _lseek(int fd, int offset, int whence) {
    return -1;
}

int _open(const char *path, int flags, int mode) {
    return syscall(3, (long)path, (long)__builtin_strlen(path), flags);
}

int _read(int fd, char *buf, int len) {
    return -1;
}

void *_sbrk(ptrdiff_t increment) {
    long ret = syscall(5, (long)increment, 0, 0);
    if (ret == -1) {
        errno = ENOMEM;
        return (void *)-1;
    }
    return (void *)ret;
}

int _write(int fd, char *buf, int len) {
    return syscall(2, (long)buf, (long)len, 0);
}

int _getpid(void) {
    return syscall(6, 0, 0, 0);
}

int _kill(int pid, int sig) {
    errno = EINVAL;
    return -1;
}
