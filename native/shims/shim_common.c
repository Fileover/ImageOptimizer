/* shim_common.c - ownership helpers shared by all shims. */
#include "io_shim.h"

#include <stdlib.h>
#include <string.h>

void io_free(void *p) { free(p); }

void io_buffer_release(io_buffer *b) {
    if (b) {
        free(b->data);
        b->data = NULL;
        b->len = 0;
    }
}

void io_image_release(io_image *img) {
    if (!img) return;
    free(img->pixels);
    img->pixels = NULL;
    img->pixels_len = 0;
    io_buffer_release(&img->icc);
    io_buffer_release(&img->exif);
    io_buffer_release(&img->xmp);
}

int io_buffer_set(io_buffer *b, const uint8_t *data, size_t len) {
    b->data = NULL;
    b->len = 0;
    if (len == 0) return 0;
    b->data = (uint8_t *)malloc(len);
    if (!b->data) return 1;
    memcpy(b->data, data, len);
    b->len = len;
    return 0;
}

void io_set_err(char *err, const char *msg) {
    if (!err) return;
    strncpy(err, msg, IO_ERR_LEN - 1);
    err[IO_ERR_LEN - 1] = '\0';
}
