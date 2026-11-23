/*
 * shim_jpeg.c - MozJPEG (libjpeg API) decode/encode used for format conversion to/from JPEG.
 * Upstream: mozilla/mozjpeg (libjpeg-turbo derived; IJG + BSD-3 + zlib licenses).
 *
 * This file is NOT derived from jpegoptim. Lossless JPEG optimization lives in
 * shim_jpegoptim.c.
 */
#include "io_shim.h"

#include <setjmp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <jpeglib.h>

typedef struct {
    struct jpeg_error_mgr pub;
    jmp_buf jb;
    char msg[JMSG_LENGTH_MAX];
} shim_err_mgr;

static void shim_error_exit(j_common_ptr c) {
    shim_err_mgr *e = (shim_err_mgr *)c->err;
    (*c->err->format_message)(c, e->msg);
    longjmp(e->jb, 1);
}
static void shim_output_message(j_common_ptr c) { (void)c; }

/* Strict mode: libjpeg normally "repairs" damaged files (truncated data is padded, bad segments
 * are skipped) and only emits a warning. A lossless optimizer must never turn a damaged file into
 * a different-looking "valid" one, so every warning is promoted to an error. */
static void shim_strict_emit(j_common_ptr c, int msg_level) {
    if (msg_level < 0) (*c->err->error_exit)(c);
}

#define SHIM_STR2(x) #x
#define SHIM_STR(x) SHIM_STR2(x)

const char *shim_jpeg_library_version(void) {
    /* MozJPEG is a libjpeg-turbo fork and reuses its version macro (mozjpeg 4.1.5 -> "4.1.5"). */
    return "mozjpeg " SHIM_STR(LIBJPEG_TURBO_VERSION);
}

#define APP_MAX 0xFFFF

static const unsigned char XMP_ID[] = "http://ns.adobe.com/xap/1.0/"; /* 28 chars + NUL */
#define XMP_ID_LEN 29
static const unsigned char ICC_ID[] = "ICC_PROFILE"; /* 11 chars + NUL */
#define ICC_ID_LEN 12

static void collect_metadata(j_decompress_ptr d, io_image *out) {
    /* ICC chunks may be split over several APP2 markers; reassemble them by sequence. */
    jpeg_saved_marker_ptr m;
    jpeg_saved_marker_ptr icc_chunk[256];
    memset(icc_chunk, 0, sizeof(icc_chunk));
    int icc_count = 0, icc_ok = 1;
    for (m = d->marker_list; m; m = m->next) {
        if (m->marker == JPEG_APP0 + 1) {
            if (m->data_length > 6 && memcmp(m->data, "Exif\0\0", 6) == 0 && !out->exif.data) {
                io_buffer_set(&out->exif, m->data + 6, m->data_length - 6);
            } else if (m->data_length > XMP_ID_LEN && memcmp(m->data, XMP_ID, XMP_ID_LEN) == 0 &&
                       !out->xmp.data) {
                io_buffer_set(&out->xmp, m->data + XMP_ID_LEN, m->data_length - XMP_ID_LEN);
            }
        } else if (m->marker == JPEG_APP0 + 2 && m->data_length > ICC_ID_LEN + 2 &&
                   memcmp(m->data, ICC_ID, ICC_ID_LEN) == 0) {
            unsigned seq = m->data[ICC_ID_LEN];
            unsigned cnt = m->data[ICC_ID_LEN + 1];
            if (seq == 0 || seq > cnt || (icc_count && (int)cnt != icc_count) || icc_chunk[seq]) {
                icc_ok = 0;
                continue;
            }
            icc_count = (int)cnt;
            icc_chunk[seq] = m;
        }
    }
    if (icc_count > 0 && icc_ok) {
        size_t total = 0;
        int i;
        for (i = 1; i <= icc_count; i++) {
            if (!icc_chunk[i]) return; /* incomplete profile: ignore */
            total += icc_chunk[i]->data_length - (ICC_ID_LEN + 2);
        }
        uint8_t *buf = (uint8_t *)malloc(total);
        if (!buf) return;
        size_t off = 0;
        for (i = 1; i <= icc_count; i++) {
            size_t n = icc_chunk[i]->data_length - (ICC_ID_LEN + 2);
            memcpy(buf + off, icc_chunk[i]->data + ICC_ID_LEN + 2, n);
            off += n;
        }
        out->icc.data = buf;
        out->icc.len = total;
    }
}

int shim_jpeg_decode(const uint8_t *data, size_t len, uint64_t max_pixels, io_image *out,
                     char *err) {
    struct jpeg_decompress_struct d;
    shim_err_mgr em;
    uint8_t *volatile pixels = NULL;
    volatile int created = 0;
    memset(out, 0, sizeof(*out));

    d.err = jpeg_std_error(&em.pub);
    em.pub.error_exit = shim_error_exit;
    em.pub.output_message = shim_output_message;
    em.pub.emit_message = shim_strict_emit;
    em.msg[0] = '\0';
    if (setjmp(em.jb)) {
        char msg[IO_ERR_LEN];
        snprintf(msg, sizeof(msg), "libjpeg: %s", em.msg);
        io_set_err(err, msg);
        if (created) jpeg_destroy_decompress(&d);
        free(pixels);
        io_image_release(out);
        return IO_ERR_CORRUPT;
    }
    jpeg_create_decompress(&d);
    created = 1;
    jpeg_save_markers(&d, JPEG_APP0 + 1, APP_MAX);
    jpeg_save_markers(&d, JPEG_APP0 + 2, APP_MAX);
    jpeg_mem_src(&d, data, (unsigned long)len);
    jpeg_read_header(&d, TRUE);

    if (d.image_width == 0 || d.image_height == 0 ||
        (uint64_t)d.image_width * (uint64_t)d.image_height > max_pixels) {
        jpeg_destroy_decompress(&d);
        io_set_err(err, "image dimensions exceed the configured pixel limit");
        return IO_ERR_TOO_LARGE;
    }

    switch (d.jpeg_color_space) {
    case JCS_GRAYSCALE: d.out_color_space = JCS_GRAYSCALE; break;
    case JCS_CMYK:
    case JCS_YCCK:
        d.out_color_space = JCS_CMYK;
        out->is_cmyk = 1;
        break;
    default: d.out_color_space = JCS_RGB; break;
    }
    /* The saved-marker list is released by jpeg_finish_decompress(): collect before decoding. */
    collect_metadata(&d, out);
    jpeg_start_decompress(&d);

    size_t stride = (size_t)d.output_width * (size_t)d.output_components;
    size_t total = stride * (size_t)d.output_height;
    pixels = (uint8_t *)malloc(total);
    if (!pixels) {
        jpeg_destroy_decompress(&d);
        io_set_err(err, "out of memory");
        return IO_ERR_OOM;
    }
    while (d.output_scanline < d.output_height) {
        JSAMPROW row = pixels + (size_t)d.output_scanline * stride;
        jpeg_read_scanlines(&d, &row, 1);
    }
    jpeg_finish_decompress(&d);

    out->width = d.output_width;
    out->height = d.output_height;
    out->channels = (uint32_t)d.output_components;
    out->bit_depth = 8;
    out->pixels = pixels;
    out->pixels_len = total;
    jpeg_destroy_decompress(&d);
    return IO_OK;
}

static void write_icc(j_compress_ptr c, const io_buffer *icc) {
    const size_t max_chunk = 65533 - (ICC_ID_LEN + 2);
    size_t chunks = (icc->len + max_chunk - 1) / max_chunk;
    size_t off = 0;
    unsigned seq;
    uint8_t *tmp = (uint8_t *)malloc(65533);
    if (!tmp || chunks > 255) {
        free(tmp);
        return;
    }
    for (seq = 1; seq <= chunks; seq++) {
        size_t n = icc->len - off < max_chunk ? icc->len - off : max_chunk;
        memcpy(tmp, ICC_ID, ICC_ID_LEN);
        tmp[ICC_ID_LEN] = (uint8_t)seq;
        tmp[ICC_ID_LEN + 1] = (uint8_t)chunks;
        memcpy(tmp + ICC_ID_LEN + 2, icc->data + off, n);
        jpeg_write_marker(c, JPEG_APP0 + 2, tmp, (unsigned)(n + ICC_ID_LEN + 2));
        off += n;
    }
    free(tmp);
}

int shim_jpeg_encode(const io_image *in, const jpeg_enc_params *p, io_buffer *out, char *err) {
    struct jpeg_compress_struct c;
    shim_err_mgr em;
    unsigned char *volatile outbuf = NULL;
    unsigned long outsize = 0;
    volatile int created = 0;
    memset(out, 0, sizeof(*out));

    if (in->bit_depth != 8 || (in->channels != 1 && in->channels != 3)) {
        io_set_err(err, "JPEG encoder requires 8-bit gray or RGB input");
        return IO_ERR_UNSUPPORTED;
    }
    if (in->width > 65535 || in->height > 65535) {
        io_set_err(err, "image dimensions exceed the JPEG limit of 65535 pixels");
        return IO_ERR_UNSUPPORTED;
    }
    c.err = jpeg_std_error(&em.pub);
    em.pub.error_exit = shim_error_exit;
    em.pub.output_message = shim_output_message;
    em.msg[0] = '\0';
    if (setjmp(em.jb)) {
        char msg[IO_ERR_LEN];
        snprintf(msg, sizeof(msg), "libjpeg: %s", em.msg);
        io_set_err(err, msg);
        if (created) jpeg_destroy_compress(&c);
        free(outbuf);
        return IO_ERR_GENERIC;
    }
    jpeg_create_compress(&c);
    created = 1;
    jpeg_mem_dest(&c, (unsigned char **)&outbuf, &outsize);
    c.image_width = in->width;
    c.image_height = in->height;
    c.input_components = (int)in->channels;
    c.in_color_space = in->channels == 1 ? JCS_GRAYSCALE : JCS_RGB;
    jpeg_set_defaults(&c); /* MozJPEG: JCP_MAX_COMPRESSION profile (trellis, optimized scans) */
    jpeg_set_quality(&c, p->quality, TRUE);
    if (!p->progressive) {
#ifdef JBOOLEAN_OPTIMIZE_SCANS
        jpeg_c_set_bool_param(&c, JBOOLEAN_OPTIMIZE_SCANS, FALSE);
#endif
        c.scan_info = NULL;
        c.num_scans = 0;
    }
    c.optimize_coding = TRUE;
    jpeg_start_compress(&c, TRUE);

    if (in->icc.len) write_icc(&c, &in->icc);
    if (in->exif.len && in->exif.len + 6 <= 65533) {
        uint8_t *tmp = (uint8_t *)malloc(in->exif.len + 6);
        if (tmp) {
            memcpy(tmp, "Exif\0\0", 6);
            memcpy(tmp + 6, in->exif.data, in->exif.len);
            jpeg_write_marker(&c, JPEG_APP0 + 1, tmp, (unsigned)(in->exif.len + 6));
            free(tmp);
        }
    }
    if (in->xmp.len && in->xmp.len + XMP_ID_LEN <= 65533) {
        uint8_t *tmp = (uint8_t *)malloc(in->xmp.len + XMP_ID_LEN);
        if (tmp) {
            memcpy(tmp, XMP_ID, XMP_ID_LEN);
            memcpy(tmp + XMP_ID_LEN, in->xmp.data, in->xmp.len);
            jpeg_write_marker(&c, JPEG_APP0 + 1, tmp, (unsigned)(in->xmp.len + XMP_ID_LEN));
            free(tmp);
        }
    }

    size_t stride = (size_t)in->width * in->channels;
    while (c.next_scanline < c.image_height) {
        JSAMPROW row = (JSAMPROW)(in->pixels + (size_t)c.next_scanline * stride);
        jpeg_write_scanlines(&c, &row, 1);
    }
    jpeg_finish_compress(&c);
    jpeg_destroy_compress(&c);
    created = 0;
    out->data = (uint8_t *)outbuf; /* allocated by libjpeg with malloc() */
    out->len = (size_t)outsize;
    return IO_OK;
}
