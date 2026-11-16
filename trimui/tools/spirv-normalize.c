/* Repair invalid Workgroup layout decorations, then run SPIRV-Tools -O.
 * Naga shares array type emission between buffer and Workgroup storage. Vulkan
 * rejects ArrayStride on ordinary Workgroup arrays (VUID-StandaloneSpirv-None-10684).
 * Remove only decorations on Workgroup types that no host-visible buffer uses.
 * The engine supplies binary SPIR-V on stdin; other CLI forms delegate unchanged.
 */
#define _GNU_SOURCE
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int normalize(uint32_t *w, size_t *count) {
    if (*count < 5 || w[0] != 0x07230203 || w[3] > 1000000) return 1;
    size_t bound = w[3];
    uint8_t *work = calloc(bound, 1), *buffer = calloc(bound, 1);
    if (!work || !buffer) { free(work); free(buffer); return 1; }
    for (size_t p=5; p<*count;) {
        unsigned n=w[p]>>16, op=w[p]&65535;
        if (!n || p+n>*count) { free(work); free(buffer); return 1; }
        if (op==32 && n==4 && w[p+3]<bound) { /* OpTypePointer */
            unsigned storage=w[p+2], type=w[p+3];
            if (storage==4) work[type]=1;
            if (storage==2 || storage==9 || storage==12) buffer[type]=1;
        }
        p+=n;
    }
    int changed;
    do {
        changed=0;
        for (size_t p=5; p<*count;) {
            unsigned n=w[p]>>16, op=w[p]&65535;
            if ((op==28 || op==29 || op==30) && n>=3 && w[p+1]<bound) {
                unsigned parent=w[p+1];
                unsigned end=op==30 ? n : 3; /* struct members, or array element */
                for (unsigned i=2;i<end;i++) if (w[p+i]<bound) {
                    unsigned child=w[p+i];
                    if (work[parent] && !work[child]) { work[child]=1; changed=1; }
                    if (buffer[parent] && !buffer[child]) { buffer[child]=1; changed=1; }
                }
            }
            p+=n;
        }
    } while(changed);
    size_t out=5;
    for (size_t p=5;p<*count;) {
        unsigned n=w[p]>>16, op=w[p]&65535;
        int layout=(op==71 && n>=3 && w[p+2]==6) || /* ArrayStride */
            (op==72 && n>=4 && (w[p+3]==35 || w[p+3]==7 || w[p+3]==4 || w[p+3]==5));
        int drop=layout && w[p+1]<bound && work[w[p+1]];
        if (drop && buffer[w[p+1]]) {
            fputs("Workgroup type aliases a buffer layout; refusing an unsafe rewrite\n",stderr);
            free(work); free(buffer); return 1;
        }
        if (!drop) { memmove(w+out,w+p,n*4); out+=n; }
        p+=n;
    }
    *count=out;
    free(work); free(buffer); return 0;
}
int main(int argc,char **argv) {
    char tool[4096];
    ssize_t len=readlink("/proc/self/exe",tool,sizeof(tool)-32);
    if (len<0) return 1;
    tool[len]=0;
    char *base=strrchr(tool,'/'); if(!base) return 1;
    strcpy(base+1,"spirv-opt");
    /* Only the engine's stdin form needs this normalization. */
    if (argc>1 && !strcmp(argv[argc-1],"-")) {
        size_t size=0, capacity=65536;
        uint8_t *data=malloc(capacity); if(!data) return 1;
        for (;;) {
            if (size==capacity) {
                if(capacity>=64*1024*1024) { free(data); return 1; }
                capacity*=2; void *next=realloc(data,capacity);
                if(!next) { free(data); return 1; } data=next;
            }
            size_t n=fread(data+size,1,capacity-size,stdin); size+=n;
            if(!n) break;
        }
        size_t count=size/4;
        if (ferror(stdin) || size%4 || normalize((uint32_t *)data,&count)) {
            fputs("Invalid shader or unsupported Workgroup layout\n",stderr); free(data); return 1;
        }
        /* tmpfile is unlinked automatically, including if the process is killed. */
        FILE *input=tmpfile(); if(!input) { free(data); return 1; }
        if(fwrite(data,4,count,input)!=count || fflush(input) || fseek(input,0,SEEK_SET) || dup2(fileno(input),STDIN_FILENO)<0) {
            fclose(input); free(data); return 1;
        }
        fclose(input); free(data);
    }
    argv[0]=tool; execv(tool,argv); perror("spirv-opt"); return 1;
}
