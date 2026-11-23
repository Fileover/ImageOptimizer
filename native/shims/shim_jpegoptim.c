/*
 * shim_jpegoptim.c - JPEG optimization core adapted from jpegoptim.
 *
 * Derived from jpegoptim (https://github.com/tjko/jpegoptim, tag v1.5.6, jpegoptim.c),
 * Copyright (C) 1996-2022 Timo Kokkonen <tjko@iki.fi>
 * SPDX-License-Identifier: GPL-3.0-or-later
 *
 * This file is part of ImageOptimizer, which as a combined work is distributed under the
 * GNU General Public License, version 3 or (at your option) any later version.
 *
 * LOCAL MODIFICATIONS relative to upstream jpegoptim.c (see also native/PATCHES.md):
 *  L-1  File/stdin/stdout handling, option parsing, logging, retry/target-size search and
 *       multi-file handling were removed. The optimization core is exposed as one function
 *       operating on memory buffers (jpeg_mem_src / jpeg_mem_dest).
 *  L-2  Error handling uses a shared setjmp/longjmp handler returning error codes instead
 *       of exiting the process.
 *  L-3  JFIF density (units, X, Y) and JFIF version are copied explicitly after
 *       jpeg_copy_critical_parameters(), because that call re-initializes them to defaults.
 *  L-4  The saved Adobe APP14 marker is not written a second time when libjpeg already
 *       emits it automatically (CMYK/YCCK), which upstream avoids only by default.
 *  L-5  Trailing data after the EOI marker is reported to the caller instead of being
 *       silently dropped, so the caller can refuse to rewrite multi-picture/appended files.
 *  L-6  Added shim_jpeg_coefficients_equal() for post-optimization verification.
 *
 * The marker classification table (jpegmarker.c) is used unmodified from upstream.
 */
#include "io_shim.h"

#include <setjmp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <jpeglib.h>
#include <jerror.h>

#include "jpegmarker.h"

typedef struct {
    struct jpeg_error_mgr pub;
    jmp_buf jb;
    char msg[JMSG_LENGTH_MAX];
} jo_err_mgr;

static void jo_error_exit(j_common_ptr c) {
    jo_err_mgr *e = (jo_err_mgr *)c->err;
    (*c->err->format_message)(c, e->msg);
    longjmp(e->jb, 1);
}
static void jo_output_message(j_common_ptr c) { (void)c; }

/* Strict mode: libjpeg normally "repairs" damaged files (truncated data is padded, bad segments
 * are skipped) and only emits a warning. A lossless optimizer must never turn a damaged file into
 * a different-looking "valid" one, so every warning is promoted to an error. */
static void jo_strict_emit(j_common_ptr c, int msg_level) {
    if (msg_level < 0) (*c->err->error_exit)(c);
}

typedef struct {
    int strip_none; /* keep every marker (jpegoptim --strip-none) */
    int save_com, save_exif, save_iptc, save_icc, save_xmp, save_jfxx, save_adobe, save_jfif;
} marker_policy;

/* Adapted from jpegoptim.c: write_markers() */
static int write_markers(struct jpeg_decompress_struct *dinfo, struct jpeg_compress_struct *cinfo,
                         const marker_policy *mp) {
    jpeg_saved_marker_ptr mrk;
    int written = 0;
    for (mrk = dinfo->marker_list; mrk; mrk = mrk->next) {
        int write_marker = 0;
        const char *s_name = jpeg_special_marker_name(mrk);

        if (mp->save_com && mrk->marker == JPEG_COM) write_marker++;
        if (mp->save_iptc && !strncmp(s_name, "IPTC", 5)) write_marker++;
        if (mp->save_exif && !strncmp(s_name, "Exif", 5)) write_marker++;
        if (mp->save_icc && !strncmp(s_name, "ICC", 4)) write_marker++;
        if (mp->save_xmp && !strncmp(s_name, "XMP", 4)) write_marker++;
        if (mp->save_jfxx && !strncmp(s_name, "JFXX", 5)) write_marker++;
        if (mp->save_adobe && !strncmp(s_name, "Adobe", 6)) write_marker++;
        if (mp->strip_none) write_marker++;

        /* libjpeg emits some markers automatically so skip these to avoid duplicates. */
        if (!strncmp(s_name, "JFIF", 5)) write_marker = 0;
        if (!strncmp(s_name, "Adobe", 6) && cinfo->write_Adobe_marker) write_marker = 0; /* L-4 */

        if (write_marker) {
            jpeg_write_marker(cinfo, mrk->marker, mrk->data, mrk->data_length);
            written++;
        }
    }
    return written;
}

static void make_policy(const jpegoptim_params *p, marker_policy *mp) {
    memset(mp, 0, sizeof(*mp));
    int any_strip = p->strip_com || p->strip_exif || p->strip_iptc || p->strip_icc || p->strip_xmp;
    mp->save_jfif = 1; /* jpegoptim default */
    if (!any_strip) {
        mp->strip_none = 1; /* keep everything */
        mp->save_com = mp->save_exif = mp->save_iptc = mp->save_icc = mp->save_xmp = 1;
    } else {
        mp->save_com = !p->strip_com;
        mp->save_exif = !p->strip_exif;
        mp->save_iptc = !p->strip_iptc;
        mp->save_icc = !p->strip_icc;
        mp->save_xmp = !p->strip_xmp;
        /* jpegoptim defaults: save_adobe = 0, save_jfxx = 0 */
    }
}

static void apply_progressive(struct jpeg_compress_struct *cinfo, int progressive_in, int mode) {
    int all_progressive = (mode == 1), all_normal = (mode == 2);
    if (all_normal || (!progressive_in && !all_progressive)) {
        /* Explicitly disable progressive mode. */
        cinfo->scan_info = NULL;
        cinfo->num_scans = 0;
    } else if (all_progressive || progressive_in) {
        jpeg_simple_progression(cinfo);
    }
}

int shim_jpegoptim_optimize(const uint8_t *data, size_t len, const jpegoptim_params *p,
                            io_buffer *out, jpegoptim_info *info, char *err) {
    struct jpeg_decompress_struct dinfo;
    struct jpeg_compress_struct cinfo;
    jo_err_mgr em;
    jvirt_barray_ptr *volatile coef_arrays = NULL;
    JSAMPROW *volatile rows = NULL;
    unsigned char *volatile outbuf = NULL;
    unsigned long outsize = 0;
    volatile int d_created = 0, c_created = 0;
    marker_policy mp;
    volatile size_t nrows = 0;

    memset(out, 0, sizeof(*out));
    memset(info, 0, sizeof(*info));
    make_policy(p, &mp);

    dinfo.err = jpeg_std_error(&em.pub);
    cinfo.err = &em.pub;
    em.pub.error_exit = jo_error_exit;
    em.pub.output_message = jo_output_message;
    em.pub.emit_message = jo_strict_emit;
    em.msg[0] = '\0';

    if (setjmp(em.jb)) {
        char msg[IO_ERR_LEN];
        snprintf(msg, sizeof(msg), "libjpeg: %s", em.msg);
        io_set_err(err, msg);
        if (c_created) jpeg_destroy_compress(&cinfo);
        if (d_created) jpeg_destroy_decompress(&dinfo);
        if (rows) {
            size_t i;
            for (i = 0; i < nrows; i++) free(rows[i]);
            free(rows);
        }
        free(outbuf);
        return IO_ERR_CORRUPT;
    }

    jpeg_create_decompress(&dinfo);
    d_created = 1;
    jpeg_create_compress(&cinfo);
    c_created = 1;

    /* Save all markers (same set as jpegoptim.c). */
    jpeg_save_markers(&dinfo, JPEG_COM, 0xffff);
    for (int i = 0; i < 16; i++) jpeg_save_markers(&dinfo, JPEG_APP0 + i, 0xffff);

    jpeg_mem_src(&dinfo, data, (unsigned long)len);
    jpeg_read_header(&dinfo, TRUE);

    if (dinfo.image_width == 0 || dinfo.image_height == 0 ||
        (uint64_t)dinfo.image_width * (uint64_t)dinfo.image_height > p->max_pixels) {
        jpeg_destroy_compress(&cinfo);
        jpeg_destroy_decompress(&dinfo);
        io_set_err(err, "image dimensions exceed the configured pixel limit");
        return IO_ERR_TOO_LARGE;
    }

    info->width = dinfo.image_width;
    info->height = dinfo.image_height;
    info->components = dinfo.num_components;
    info->progressive_in = dinfo.progressive_mode ? 1 : 0;
    info->arithmetic_in = dinfo.arith_code ? 1 : 0;
    info->saw_jfif = dinfo.saw_JFIF_marker ? 1 : 0;
    info->saw_adobe = dinfo.saw_Adobe_marker ? 1 : 0;
    {
        jpeg_saved_marker_ptr m;
        for (m = dinfo.marker_list; m; m = m->next) info->markers_in++;
    }

    if (p->quality >= 0) {
        /* Lossy "optimization": decompress to pixels and recompress (jpegoptim -m<q>). */
        jpeg_start_decompress(&dinfo);
        nrows = dinfo.output_height;
        rows = (JSAMPROW *)calloc(nrows, sizeof(JSAMPROW));
        if (!rows) {
            nrows = 0;
            ERREXIT1(&dinfo, JERR_OUT_OF_MEMORY, 0);
        }
        size_t stride = (size_t)dinfo.output_width * (size_t)dinfo.out_color_components;
        for (size_t i = 0; i < nrows; i++) {
            rows[i] = (JSAMPROW)calloc(stride, sizeof(JSAMPLE));
            if (!rows[i]) {
                nrows = i;
                ERREXIT1(&dinfo, JERR_OUT_OF_MEMORY, 0);
            }
        }
        while (dinfo.output_scanline < dinfo.output_height) {
            jpeg_read_scanlines(&dinfo, &rows[dinfo.output_scanline],
                                dinfo.output_height - dinfo.output_scanline);
        }
    } else {
        coef_arrays = jpeg_read_coefficients(&dinfo);
        if (!coef_arrays) {
            ERREXIT(&dinfo, JERR_NO_IMAGE); /* failed to read coefficients */
        }
        /* L-5: everything after EOI is reported, never silently dropped. */
        info->trailing_bytes = (size_t)dinfo.src->bytes_in_buffer;
        {
            size_t i;
            for (i = 0; i < info->trailing_bytes; i++) {
                if (dinfo.src->next_input_byte[i] != 0) {
                    info->trailing_nonzero = 1;
                    break;
                }
            }
        }
    }

    jpeg_mem_dest(&cinfo, (unsigned char **)&outbuf, &outsize);

    if (p->quality >= 0) {
        cinfo.in_color_space = dinfo.out_color_space;
        cinfo.input_components = dinfo.output_components;
        cinfo.image_width = dinfo.image_width;
        cinfo.image_height = dinfo.image_height;
        jpeg_set_defaults(&cinfo);
        jpeg_set_quality(&cinfo, p->quality, TRUE);
#ifdef JINT_DC_SCAN_OPT_MODE
        if (jpeg_c_int_param_supported(&cinfo, JINT_DC_SCAN_OPT_MODE))
            jpeg_c_set_int_param(&cinfo, JINT_DC_SCAN_OPT_MODE, 1);
#endif
        apply_progressive(&cinfo, dinfo.progressive_mode, p->progressive_mode);
        cinfo.optimize_coding = TRUE;
        cinfo.write_JFIF_header = (dinfo.saw_JFIF_marker && (mp.save_jfif || mp.strip_none)) ? TRUE : FALSE;
        if (dinfo.saw_Adobe_marker && (mp.save_adobe || mp.strip_none)) cinfo.write_JFIF_header = FALSE;
        if (cinfo.write_JFIF_header) {
            cinfo.density_unit = dinfo.density_unit; /* L-3 */
            cinfo.X_density = dinfo.X_density;
            cinfo.Y_density = dinfo.Y_density;
        }

        jpeg_start_compress(&cinfo, TRUE);
        info->markers_written = write_markers(&dinfo, &cinfo, &mp);
        while (cinfo.next_scanline < cinfo.image_height) {
            jpeg_write_scanlines(&cinfo, &rows[cinfo.next_scanline], dinfo.output_height);
        }
    } else {
        /* Lossless optimization. */
        jpeg_copy_critical_parameters(&dinfo, &cinfo);
#ifdef JINT_DC_SCAN_OPT_MODE
        if (jpeg_c_int_param_supported(&cinfo, JINT_DC_SCAN_OPT_MODE))
            jpeg_c_set_int_param(&cinfo, JINT_DC_SCAN_OPT_MODE, 1);
#endif
        apply_progressive(&cinfo, dinfo.progressive_mode, p->progressive_mode);
        cinfo.optimize_coding = TRUE;
        cinfo.arith_code = FALSE; /* never emit arithmetic coding (poor decoder support) */
        cinfo.write_JFIF_header = (dinfo.saw_JFIF_marker && (mp.save_jfif || mp.strip_none)) ? TRUE : FALSE;
        if (dinfo.saw_Adobe_marker && (mp.save_adobe || mp.strip_none)) cinfo.write_JFIF_header = FALSE;
        if (cinfo.write_JFIF_header) {
            cinfo.density_unit = dinfo.density_unit; /* L-3 */
            cinfo.X_density = dinfo.X_density;
            cinfo.Y_density = dinfo.Y_density;
            cinfo.JFIF_major_version = dinfo.JFIF_major_version;
            cinfo.JFIF_minor_version = dinfo.JFIF_minor_version;
        }
        jpeg_write_coefficients(&cinfo, coef_arrays);
        info->markers_written = write_markers(&dinfo, &cinfo, &mp);
    }
    jpeg_finish_compress(&cinfo);

    if (p->quality >= 0) {
        jpeg_finish_decompress(&dinfo);
        info->trailing_bytes = (size_t)dinfo.src->bytes_in_buffer;
        size_t i;
        for (i = 0; i < info->trailing_bytes; i++) {
            if (dinfo.src->next_input_byte[i] != 0) {
                info->trailing_nonzero = 1;
                break;
            }
        }
        for (i = 0; i < nrows; i++) free(rows[i]);
        free(rows);
        rows = NULL;
        nrows = 0;
    }

    out->data = (uint8_t *)outbuf; /* malloc()ed by libjpeg */
    out->len = (size_t)outsize;
    outbuf = NULL;
    jpeg_destroy_compress(&cinfo);
    jpeg_destroy_decompress(&dinfo);
    return IO_OK;
}

/* ---------------------------------------------------------------- verification (L-6) */

static int markers_equal(struct jpeg_decompress_struct *a, struct jpeg_decompress_struct *b) {
    jpeg_saved_marker_ptr ma = a->marker_list, mb = b->marker_list;
    for (;;) {
        while (ma && !strncmp(jpeg_special_marker_name(ma), "JFIF", 5)) ma = ma->next;
        while (mb && !strncmp(jpeg_special_marker_name(mb), "JFIF", 5)) mb = mb->next;
        if (!ma || !mb) return ma == mb;
        if (ma->marker != mb->marker || ma->data_length != mb->data_length ||
            memcmp(ma->data, mb->data, ma->data_length) != 0)
            return 0;
        ma = ma->next;
        mb = mb->next;
    }
}

int shim_jpeg_coefficients_equal(const uint8_t *a, size_t alen, const uint8_t *b, size_t blen,
                                 int compare_markers, int *equal, char *err) {
    struct jpeg_decompress_struct da, db;
    jo_err_mgr ea, eb_unused;
    volatile int a_created = 0, b_created = 0;
    (void)eb_unused;
    *equal = 0;

    da.err = jpeg_std_error(&ea.pub);
    db.err = &ea.pub;
    ea.pub.error_exit = jo_error_exit;
    ea.pub.output_message = jo_output_message;
    ea.pub.emit_message = jo_strict_emit;
    ea.msg[0] = '\0';
    if (setjmp(ea.jb)) {
        char msg[IO_ERR_LEN];
        snprintf(msg, sizeof(msg), "libjpeg: %s", ea.msg);
        io_set_err(err, msg);
        if (a_created) jpeg_destroy_decompress(&da);
        if (b_created) jpeg_destroy_decompress(&db);
        return IO_ERR_CORRUPT;
    }
    jpeg_create_decompress(&da);
    a_created = 1;
    jpeg_create_decompress(&db);
    b_created = 1;
    jpeg_save_markers(&da, JPEG_COM, 0xffff);
    jpeg_save_markers(&db, JPEG_COM, 0xffff);
    for (int i = 0; i < 16; i++) {
        jpeg_save_markers(&da, JPEG_APP0 + i, 0xffff);
        jpeg_save_markers(&db, JPEG_APP0 + i, 0xffff);
    }
    jpeg_mem_src(&da, a, (unsigned long)alen);
    jpeg_mem_src(&db, b, (unsigned long)blen);
    jpeg_read_header(&da, TRUE);
    jpeg_read_header(&db, TRUE);
    jvirt_barray_ptr *ca = jpeg_read_coefficients(&da);
    jvirt_barray_ptr *cb = jpeg_read_coefficients(&db);

    int same = 1;
    if (da.image_width != db.image_width || da.image_height != db.image_height ||
        da.num_components != db.num_components || da.jpeg_color_space != db.jpeg_color_space ||
        da.data_precision != db.data_precision)
        same = 0;
    for (int c = 0; same && c < da.num_components; c++) {
        jpeg_component_info *x = &da.comp_info[c], *y = &db.comp_info[c];
        if (x->h_samp_factor != y->h_samp_factor || x->v_samp_factor != y->v_samp_factor ||
            x->width_in_blocks != y->width_in_blocks || x->height_in_blocks != y->height_in_blocks ||
            !x->quant_table || !y->quant_table ||
            memcmp(x->quant_table->quantval, y->quant_table->quantval, sizeof(x->quant_table->quantval)) != 0) {
            same = 0;
            break;
        }
        for (JDIMENSION r = 0; r < x->height_in_blocks; r++) {
            JBLOCKARRAY ra = (*da.mem->access_virt_barray)((j_common_ptr)&da, ca[c], r, 1, FALSE);
            JBLOCKARRAY rb = (*db.mem->access_virt_barray)((j_common_ptr)&db, cb[c], r, 1, FALSE);
            if (memcmp(ra[0], rb[0], (size_t)x->width_in_blocks * sizeof(JBLOCK)) != 0) {
                same = 0;
                break;
            }
        }
    }
    if (same && compare_markers) same = markers_equal(&da, &db);
    *equal = same;
    jpeg_destroy_decompress(&da);
    jpeg_destroy_decompress(&db);
    return IO_OK;
}
