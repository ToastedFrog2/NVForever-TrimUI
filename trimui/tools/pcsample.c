// pcsample: statistical sampler for one thread of a running process (TrimUI profiling).
// Usage: pcsample <tid> <samples> <interval_us>. Prints "pc lr" in hex, one sample per line.
// Each sample seizes the thread with ptrace, interrupts it, reads PC and LR (x30), detaches.
#define _GNU_SOURCE
#include <elf.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/ptrace.h>
#include <sys/uio.h>
#include <sys/wait.h>
#include <unistd.h>

struct regs { uint64_t x[31]; uint64_t sp, pc, pstate; };

int main(int argc, char **argv) {
    if (argc < 4) { fprintf(stderr, "usage: pcsample tid samples interval_us\n"); return 2; }
    pid_t tid = atoi(argv[1]);
    int n = atoi(argv[2]);
    useconds_t gap = (useconds_t)atoi(argv[3]);
    for (int i = 0; i < n; i++) {
        if (ptrace(PTRACE_SEIZE, tid, 0, 0) != 0) { perror("seize"); return 1; }
        ptrace(PTRACE_INTERRUPT, tid, 0, 0);
        int st;
        if (waitpid(tid, &st, __WALL) < 0) { perror("wait"); return 1; }
        struct regs r;
        struct iovec io = { &r, sizeof r };
        if (ptrace(PTRACE_GETREGSET, tid, (void *)NT_PRSTATUS, &io) == 0) printf("%llx %llx\n", (unsigned long long)r.pc, (unsigned long long)r.x[30]);
        ptrace(PTRACE_DETACH, tid, 0, 0);
        usleep(gap);
    }
    return 0;
}
