/* Small framebuffer loading card: no graphics context or extra runtime libraries. */
#include <fcntl.h>
#include <linux/fb.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>

static const uint8_t font[26][7] = {
 {14,17,17,31,17,17,17}, {30,17,17,30,17,17,30}, {14,17,16,16,16,17,14},
 {30,17,17,17,17,17,30}, {31,16,16,30,16,16,31}, {31,16,16,30,16,16,16},
 {14,17,16,23,17,17,15}, {17,17,17,31,17,17,17}, {14,4,4,4,4,4,14},
 {7,2,2,2,2,18,12}, {17,18,20,24,20,18,17}, {16,16,16,16,16,16,31},
 {17,27,21,21,17,17,17}, {17,25,21,19,17,17,17}, {14,17,17,17,17,17,14},
 {30,17,17,30,16,16,16}, {14,17,17,17,21,18,13}, {30,17,17,30,20,18,17},
 {15,16,16,14,1,1,30}, {31,4,4,4,4,4,4}, {17,17,17,17,17,17,14},
 {17,17,17,17,17,10,4}, {17,17,17,21,21,21,10}, {17,17,10,4,10,17,17},
 {17,17,10,4,4,4,4}, {31,1,2,4,8,16,31},
};
static uint32_t *pixels;
static unsigned width, height, pitch;
static void text(const char *s, unsigned y, unsigned scale, uint32_t color) {
    unsigned len = strlen(s), x0 = (width - len * 6 * scale) / 2;
    for (unsigned i=0; i<len; ++i) {
        if (s[i] < 'A' || s[i] > 'Z') continue;
        for (unsigned r=0; r<7; ++r) for (unsigned c=0; c<5; ++c) {
            if (!(font[s[i]-'A'][r] & (16 >> c))) continue;
            for (unsigned yy=0; yy<scale; ++yy) for (unsigned xx=0; xx<scale; ++xx) {
                unsigned x=x0+(i*6+c)*scale+xx, py=y+r*scale+yy;
                if (x<width && py<height) pixels[py*pitch+x]=color;
            }
        }
    }
}
int main(void) {
    int fd=open("/dev/fb0",O_RDWR);
    struct fb_var_screeninfo v;
    struct fb_fix_screeninfo f;
    if (fd<0 || ioctl(fd,FBIOGET_VSCREENINFO,&v) || ioctl(fd,FBIOGET_FSCREENINFO,&f) || v.bits_per_pixel!=32) return 1;
    size_t bytes=(v.yoffset+v.yres)*f.line_length;
    void *map=mmap(NULL,bytes,PROT_READ|PROT_WRITE,MAP_SHARED,fd,0);
    if (map==MAP_FAILED) { close(fd); return 2; }
    pixels=(uint32_t *)((char *)map+v.yoffset*f.line_length);
    width=v.xres; height=v.yres; pitch=f.line_length/4;
    uint32_t alpha=255u<<v.transp.offset;
    uint32_t bg=alpha|(12u<<v.red.offset)|(17u<<v.green.offset)|(20u<<v.blue.offset);
    uint32_t amber=alpha|(235u<<v.red.offset)|(182u<<v.green.offset)|(92u<<v.blue.offset);
    uint32_t white=alpha|(180u<<v.red.offset)|(189u<<v.green.offset)|(185u<<v.blue.offset);
    for (unsigned y=0;y<height;++y) for(unsigned x=0;x<width;++x) pixels[y*pitch+x]=bg;
    text("NVFOREVER",height/2-115,8,amber);
    text("LOADING THE MOJAVE",height/2-5,4,white);
    text("FIRST START MAY TAKE A FEW MINUTES",height/2+75,3,white);
    msync(map,bytes,MS_SYNC);
    v.activate=FB_ACTIVATE_VBL;
    if (ioctl(fd,FBIOPAN_DISPLAY,&v)) perror("loading framebuffer present");
    munmap(map,bytes); close(fd);
    return 0;
}
