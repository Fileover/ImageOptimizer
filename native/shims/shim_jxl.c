/*
 * shim_jxl.c - libjxl integration through the public C API (jxl/encode.h, jxl/decode.h,
 * jxl/thread_parallel_runner.h). Upstream: libjxl/libjxl.
 */
#include "io_shim.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <jxl/codestream_header.h>
#include <jxl/color_encoding.h>
#include <jxl/decode.h>
#include <jxl/encode.h>
#include <jxl/thread_parallel_runner.h>
#include <jxl/types.h>

uint32_t shim_jxl_encoder_version(void) { return JxlEncoderVersion(); }
uint32_t shim_jxl_decoder_version(void) { return JxlDecoderVersion(); }
float shim_jxl_distance_from_quality(float quality) {
    return JxlEncoderDistanceFromQuality(quality);
}

static size_t worker_threads(int requested) {
    if (requested > 0) return (size_t)requested;
    return JxlThreadParallelRunnerDefaultNumWorkerThreads();
}

static const char *enc_err_name(JxlEncoderError e) {
    switch (e) {
    case JXL_ENC_ERR_OK: return "ok";
    case JXL_ENC_ERR_GENERIC: return "generic encoder error";
    case JXL_ENC_ERR_OOM: return "out of memory";
    case JXL_ENC_ERR_JBRD: return "JPEG bitstream reconstruction data could not be created";
    case JXL_ENC_ERR_BAD_INPUT: return "bad input (unsupported or malformed image/JPEG)";
    case JXL_ENC_ERR_NOT_SUPPORTED: return "feature not supported by this libjxl build";
    case JXL_ENC_ERR_API_USAGE: return "invalid API usage / unsupported parameter combination";
    default: return "unknown encoder error";
    }
}

/* Drains JxlEncoderProcessOutput into a growing malloc'ed buffer. */
static int drain_encoder(JxlEncoder *enc, io_buffer *out, char *err) {
    size_t cap = 1u << 16, used = 0;
    uint8_t *buf = (uint8_t *)malloc(cap);
    if (!buf) {
        io_set_err(err, "out of memory");
        return IO_ERR_OOM;
    }
    JxlEncoderStatus st;
    do {
        uint8_t *next = buf + used;
        size_t avail = cap - used;
        st = JxlEncoderProcessOutput(enc, &next, &avail);
        used = (size_t)(next - buf);
        if (st == JXL_ENC_NEED_MORE_OUTPUT) {
            size_t ncap = cap * 2;
            uint8_t *nb = (uint8_t *)realloc(buf, ncap);
            if (!nb) {
                free(buf);
                io_set_err(err, "out of memory");
                return IO_ERR_OOM;
            }
            buf = nb;
            cap = ncap;
        }
    } while (st == JXL_ENC_NEED_MORE_OUTPUT);
    if (st != JXL_ENC_SUCCESS) {
        free(buf);
        char msg[IO_ERR_LEN];
        snprintf(msg, sizeof(msg), "JxlEncoderProcessOutput failed: %s",
                 enc_err_name(JxlEncoderGetError(enc)));
        io_set_err(err, msg);
        return IO_ERR_GENERIC;
    }
    out->data = buf;
    out->len = used;
    return IO_OK;
}

static int enc_fail(JxlEncoder *enc, const char *what, char *err) {
    char msg[IO_ERR_LEN];
    snprintf(msg, sizeof(msg), "%s: %s", what, enc_err_name(JxlEncoderGetError(enc)));
    io_set_err(err, msg);
    return IO_ERR_GENERIC;
}

/* EXIF box payload = 4-byte big-endian TIFF header offset (0) + TIFF block. */
static int add_box(JxlEncoder *enc, const char type[4], const uint8_t *prefix4,
                   const io_buffer *payload) {
    size_t extra = prefix4 ? 4 : 0;
    uint8_t *tmp = (uint8_t *)malloc(payload->len + extra);
    if (!tmp) return 0;
    if (prefix4) memcpy(tmp, prefix4, 4);
    memcpy(tmp + extra, payload->data, payload->len);
    JxlEncoderStatus st = JxlEncoderAddBox(enc, type, tmp, payload->len + extra, JXL_TRUE);
    free(tmp);
    return st == JXL_ENC_SUCCESS;
}

int shim_jxl_encode(const io_image *in, const jxl_params *p, io_buffer *out, char *err) {
    memset(out, 0, sizeof(*out));
    if ((in->bit_depth != 8 && in->bit_depth != 16) || in->channels < 1 || in->channels > 4) {
        io_set_err(err, "JPEG XL encoder requires 8/16-bit gray, gray+alpha, RGB or RGBA input");
        return IO_ERR_UNSUPPORTED;
    }
    int rc = IO_ERR_GENERIC;
    JxlEncoder *enc = JxlEncoderCreate(NULL);
    void *runner = NULL;
    if (!enc) {
        io_set_err(err, "JxlEncoderCreate failed");
        return IO_ERR_OOM;
    }
    runner = JxlThreadParallelRunnerCreate(NULL, worker_threads(p->threads));
    if (!runner || JxlEncoderSetParallelRunner(enc, JxlThreadParallelRunner, runner) !=
                       JXL_ENC_SUCCESS) {
        rc = enc_fail(enc, "JxlEncoderSetParallelRunner", err);
        goto done;
    }

    int has_alpha = (in->channels == 2 || in->channels == 4);
    int is_gray = (in->channels <= 2);
    int want_boxes = p->store_metadata && (in->exif.len || in->xmp.len);
    if (want_boxes && JxlEncoderUseBoxes(enc) != JXL_ENC_SUCCESS) {
        rc = enc_fail(enc, "JxlEncoderUseBoxes", err);
        goto done;
    }

    JxlBasicInfo info;
    JxlEncoderInitBasicInfo(&info);
    info.xsize = in->width;
    info.ysize = in->height;
    info.bits_per_sample = in->bit_depth;
    info.exponent_bits_per_sample = 0;
    info.num_color_channels = is_gray ? 1 : 3;
    info.alpha_bits = has_alpha ? in->bit_depth : 0;
    info.alpha_exponent_bits = 0;
    info.num_extra_channels = has_alpha ? 1 : 0;
    /* Lossless encoding requires the original color profile to be kept as-is. */
    info.uses_original_profile = p->lossless ? JXL_TRUE : JXL_FALSE;
    if (p->orientation >= 1 && p->orientation <= 8) info.orientation = (JxlOrientation)p->orientation;
    if (JxlEncoderSetBasicInfo(enc, &info) != JXL_ENC_SUCCESS) {
        rc = enc_fail(enc, "JxlEncoderSetBasicInfo", err);
        goto done;
    }

    if (in->icc.len) {
        if (JxlEncoderSetICCProfile(enc, in->icc.data, in->icc.len) != JXL_ENC_SUCCESS) {
            rc = enc_fail(enc, "JxlEncoderSetICCProfile (invalid ICC profile?)", err);
            goto done;
        }
    } else {
        JxlColorEncoding ce;
        JxlColorEncodingSetToSRGB(&ce, is_gray ? JXL_TRUE : JXL_FALSE);
        if (JxlEncoderSetColorEncoding(enc, &ce) != JXL_ENC_SUCCESS) {
            rc = enc_fail(enc, "JxlEncoderSetColorEncoding", err);
            goto done;
        }
    }

    if (p->effort > 10) {
        JxlEncoderAllowExpertOptions(enc);
    }
    JxlEncoderFrameSettings *fs = JxlEncoderFrameSettingsCreate(enc, NULL);
    if (!fs) {
        rc = enc_fail(enc, "JxlEncoderFrameSettingsCreate", err);
        goto done;
    }
    if (JxlEncoderFrameSettingsSetOption(fs, JXL_ENC_FRAME_SETTING_EFFORT, p->effort) !=
        JXL_ENC_SUCCESS) {
        rc = enc_fail(enc, "setting effort", err);
        goto done;
    }
    if (p->brotli_effort >= 0 &&
        JxlEncoderFrameSettingsSetOption(fs, JXL_ENC_FRAME_SETTING_BROTLI_EFFORT,
                                         p->brotli_effort) != JXL_ENC_SUCCESS) {
        rc = enc_fail(enc, "setting brotli effort", err);
        goto done;
    }
    if (p->lossless) {
        if (JxlEncoderSetFrameLossless(fs, JXL_TRUE) != JXL_ENC_SUCCESS) {
            rc = enc_fail(enc, "JxlEncoderSetFrameLossless", err);
            goto done;
        }
    } else if (JxlEncoderSetFrameDistance(fs, p->distance) != JXL_ENC_SUCCESS) {
        rc = enc_fail(enc, "JxlEncoderSetFrameDistance", err);
        goto done;
    }

    JxlPixelFormat pf;
    pf.num_channels = in->channels;
    pf.data_type = in->bit_depth == 16 ? JXL_TYPE_UINT16 : JXL_TYPE_UINT8;
    pf.endianness = JXL_NATIVE_ENDIAN;
    pf.align = 0;
    if (JxlEncoderAddImageFrame(fs, &pf, in->pixels, in->pixels_len) != JXL_ENC_SUCCESS) {
        rc = enc_fail(enc, "JxlEncoderAddImageFrame", err);
        goto done;
    }

    if (p->store_metadata) {
        static const uint8_t zero_offset[4] = {0, 0, 0, 0};
        if (in->exif.len && !add_box(enc, "Exif", zero_offset, &in->exif)) {
            rc = enc_fail(enc, "adding Exif box", err);
            goto done;
        }
        if (in->xmp.len && !add_box(enc, "xml ", NULL, &in->xmp)) {
            rc = enc_fail(enc, "adding XMP box", err);
            goto done;
        }
    }
    JxlEncoderCloseInput(enc);
    rc = drain_encoder(enc, out, err);
done:
    if (runner) JxlThreadParallelRunnerDestroy(runner);
    JxlEncoderDestroy(enc);
    return rc;
}

int shim_jxl_encode_jpeg(const uint8_t *jpeg, size_t len, const jxl_params *p, io_buffer *out,
                         char *err) {
    memset(out, 0, sizeof(*out));
    int rc = IO_ERR_GENERIC;
    JxlEncoder *enc = JxlEncoderCreate(NULL);
    void *runner = NULL;
    if (!enc) {
        io_set_err(err, "JxlEncoderCreate failed");
        return IO_ERR_OOM;
    }
    runner = JxlThreadParallelRunnerCreate(NULL, worker_threads(p->threads));
    if (!runner || JxlEncoderSetParallelRunner(enc, JxlThreadParallelRunner, runner) !=
                       JXL_ENC_SUCCESS) {
        rc = enc_fail(enc, "JxlEncoderSetParallelRunner", err);
        goto done;
    }
    /* Keep everything needed to rebuild the original JPEG byte-for-byte. */
    if (JxlEncoderStoreJPEGMetadata(enc, JXL_TRUE) != JXL_ENC_SUCCESS) {
        rc = enc_fail(enc, "JxlEncoderStoreJPEGMetadata", err);
        goto done;
    }
    if (p->effort > 10) {
        JxlEncoderAllowExpertOptions(enc);
    }
    JxlEncoderFrameSettings *fs = JxlEncoderFrameSettingsCreate(enc, NULL);
    if (!fs) {
        rc = enc_fail(enc, "JxlEncoderFrameSettingsCreate", err);
        goto done;
    }
    if (JxlEncoderFrameSettingsSetOption(fs, JXL_ENC_FRAME_SETTING_EFFORT, p->effort) !=
        JXL_ENC_SUCCESS) {
        rc = enc_fail(enc, "setting effort", err);
        goto done;
    }
    if (p->brotli_effort >= 0 &&
        JxlEncoderFrameSettingsSetOption(fs, JXL_ENC_FRAME_SETTING_BROTLI_EFFORT,
                                         p->brotli_effort) != JXL_ENC_SUCCESS) {
        rc = enc_fail(enc, "setting brotli effort", err);
        goto done;
    }
    if (JxlEncoderAddJPEGFrame(fs, jpeg, len) != JXL_ENC_SUCCESS) {
        rc = enc_fail(enc, "JxlEncoderAddJPEGFrame (JPEG not eligible for lossless transcoding)",
                      err);
        goto done;
    }
    JxlEncoderCloseInput(enc);
    rc = drain_encoder(enc, out, err);
done:
    if (runner) JxlThreadParallelRunnerDestroy(runner);
    JxlEncoderDestroy(enc);
    return rc;
}

/* ------------------------------------------------------------------ decoding */

typedef struct {
    uint8_t *buf;
    size_t cap;
    char type[4];
    int active;
} box_state;

static void keep_box(io_image *out, box_state *bs, size_t used) {
    if (!bs->active) return;
    io_buffer *dst = NULL;
    if (memcmp(bs->type, "Exif", 4) == 0) dst = &out->exif;
    else if (memcmp(bs->type, "xml ", 4) == 0) dst = &out->xmp;
    if (dst && used > 0 && dst->data == NULL) {
        if (dst == &out->exif) {
            /* skip the 4-byte offset prefix and any padding before the TIFF header */
            if (used >= 4) {
                uint32_t off = ((uint32_t)bs->buf[0] << 24) | ((uint32_t)bs->buf[1] << 16) |
                               ((uint32_t)bs->buf[2] << 8) | (uint32_t)bs->buf[3];
                if ((size_t)off + 4 <= used) io_buffer_set(dst, bs->buf + 4 + off, used - 4 - off);
            }
        } else {
            io_buffer_set(dst, bs->buf, used);
        }
    }
    free(bs->buf);
    bs->buf = NULL;
    bs->cap = 0;
    bs->active = 0;
}

static int is_srgb_encoding(const JxlColorEncoding *ce) {
    if (ce->white_point != JXL_WHITE_POINT_D65) return 0;
    if (ce->transfer_function != JXL_TRANSFER_FUNCTION_SRGB) return 0;
    if (ce->color_space == JXL_COLOR_SPACE_GRAY) return 1;
    return ce->color_space == JXL_COLOR_SPACE_RGB && ce->primaries == JXL_PRIMARIES_SRGB;
}

int shim_jxl_decode(const uint8_t *data, size_t len, uint64_t max_pixels, int threads,
                    int keep_orientation, io_image *out, char *err) {
    memset(out, 0, sizeof(*out));
    int rc = IO_ERR_GENERIC;
    box_state bs;
    memset(&bs, 0, sizeof(bs));
    JxlDecoder *dec = JxlDecoderCreate(NULL);
    void *runner = NULL;
    if (!dec) {
        io_set_err(err, "JxlDecoderCreate failed");
        return IO_ERR_OOM;
    }
    runner = JxlThreadParallelRunnerCreate(NULL, worker_threads(threads));
    if (!runner || JxlDecoderSetParallelRunner(dec, JxlThreadParallelRunner, runner) !=
                       JXL_DEC_SUCCESS) {
        io_set_err(err, "JxlDecoderSetParallelRunner failed");
        goto done;
    }
    if (JxlDecoderSubscribeEvents(dec, JXL_DEC_BASIC_INFO | JXL_DEC_COLOR_ENCODING |
                                           JXL_DEC_FULL_IMAGE | JXL_DEC_BOX) != JXL_DEC_SUCCESS) {
        io_set_err(err, "JxlDecoderSubscribeEvents failed");
        goto done;
    }
    JxlDecoderSetDecompressBoxes(dec, JXL_TRUE);
    /* default (keep_orientation = 0): orientation is applied, so the output is upright. */
    JxlDecoderSetKeepOrientation(dec, keep_orientation ? JXL_TRUE : JXL_FALSE);
    if (JxlDecoderSetInput(dec, data, len) != JXL_DEC_SUCCESS) {
        io_set_err(err, "JxlDecoderSetInput failed");
        goto done;
    }
    JxlDecoderCloseInput(dec);

    JxlBasicInfo info;
    JxlPixelFormat pf;
    int have_info = 0, have_image = 0;
    for (;;) {
        JxlDecoderStatus st = JxlDecoderProcessInput(dec);
        if (st == JXL_DEC_SUCCESS) break;
        if (st == JXL_DEC_ERROR) {
            io_set_err(err, "libjxl reported a decoding error (corrupt or truncated file)");
            rc = IO_ERR_CORRUPT;
            goto done;
        }
        if (st == JXL_DEC_NEED_MORE_INPUT) {
            io_set_err(err, "truncated JPEG XL file");
            rc = IO_ERR_CORRUPT;
            goto done;
        }
        if (st == JXL_DEC_BASIC_INFO) {
            if (JxlDecoderGetBasicInfo(dec, &info) != JXL_DEC_SUCCESS) {
                io_set_err(err, "JxlDecoderGetBasicInfo failed");
                goto done;
            }
            have_info = 1;
            if (info.have_animation) {
                out->is_animated = 1;
                io_set_err(err, "animated JPEG XL is not supported (animation would be discarded)");
                rc = IO_ERR_UNSUPPORTED;
                goto done;
            }
            if (info.exponent_bits_per_sample > 0) {
                io_set_err(err, "floating point JPEG XL images are not supported");
                rc = IO_ERR_UNSUPPORTED;
                goto done;
            }
            int has_alpha = info.alpha_bits > 0;
            if ((int)info.num_extra_channels > (has_alpha ? 1 : 0)) {
                io_set_err(err, "JPEG XL extra channels other than alpha are not supported");
                rc = IO_ERR_UNSUPPORTED;
                goto done;
            }
            if ((uint64_t)info.xsize * (uint64_t)info.ysize > max_pixels) {
                io_set_err(err, "image dimensions exceed the configured pixel limit");
                rc = IO_ERR_TOO_LARGE;
                goto done;
            }
            if (info.alpha_premultiplied) JxlDecoderSetUnpremultiplyAlpha(dec, JXL_TRUE);
            pf.num_channels = info.num_color_channels + (has_alpha ? 1 : 0);
            pf.data_type = info.bits_per_sample > 8 ? JXL_TYPE_UINT16 : JXL_TYPE_UINT8;
            pf.endianness = JXL_NATIVE_ENDIAN;
            pf.align = 0;
            out->width = info.xsize;
            out->height = info.ysize;
            out->channels = pf.num_channels;
            out->bit_depth = info.bits_per_sample > 8 ? 16 : 8;
        } else if (st == JXL_DEC_COLOR_ENCODING) {
            JxlColorEncoding ce;
            int want_icc = 1;
            if (JxlDecoderGetColorAsEncodedProfile(dec, JXL_COLOR_PROFILE_TARGET_ORIGINAL,
                                                   &ce) == JXL_DEC_SUCCESS &&
                is_srgb_encoding(&ce)) {
                want_icc = 0; /* plain sRGB: no ICC profile needs to be carried */
            }
            if (want_icc) {
                size_t sz = 0;
                if (JxlDecoderGetICCProfileSize(dec, JXL_COLOR_PROFILE_TARGET_ORIGINAL, &sz) ==
                        JXL_DEC_SUCCESS &&
                    sz > 0) {
                    out->icc.data = (uint8_t *)malloc(sz);
                    if (!out->icc.data) {
                        io_set_err(err, "out of memory");
                        rc = IO_ERR_OOM;
                        goto done;
                    }
                    if (JxlDecoderGetColorAsICCProfile(dec, JXL_COLOR_PROFILE_TARGET_ORIGINAL,
                                                       out->icc.data, sz) != JXL_DEC_SUCCESS) {
                        io_set_err(err, "could not read the ICC profile");
                        goto done;
                    }
                    out->icc.len = sz;
                }
            }
        } else if (st == JXL_DEC_NEED_IMAGE_OUT_BUFFER) {
            size_t sz = 0;
            if (!have_info || JxlDecoderImageOutBufferSize(dec, &pf, &sz) != JXL_DEC_SUCCESS) {
                io_set_err(err, "JxlDecoderImageOutBufferSize failed");
                goto done;
            }
            out->pixels = (uint8_t *)malloc(sz);
            if (!out->pixels) {
                io_set_err(err, "out of memory");
                rc = IO_ERR_OOM;
                goto done;
            }
            out->pixels_len = sz;
            if (JxlDecoderSetImageOutBuffer(dec, &pf, out->pixels, sz) != JXL_DEC_SUCCESS) {
                io_set_err(err, "JxlDecoderSetImageOutBuffer failed");
                goto done;
            }
        } else if (st == JXL_DEC_FULL_IMAGE) {
            have_image = 1;
        } else if (st == JXL_DEC_BOX) {
            if (bs.active) {
                size_t remaining = JxlDecoderReleaseBoxBuffer(dec);
                keep_box(out, &bs, bs.cap - remaining);
            }
            char type[4];
            if (JxlDecoderGetBoxType(dec, type, JXL_TRUE) != JXL_DEC_SUCCESS) {
                io_set_err(err, "JxlDecoderGetBoxType failed");
                goto done;
            }
            if (memcmp(type, "Exif", 4) == 0 || memcmp(type, "xml ", 4) == 0) {
                memcpy(bs.type, type, 4);
                bs.cap = 1u << 16;
                bs.buf = (uint8_t *)malloc(bs.cap);
                if (!bs.buf) {
                    io_set_err(err, "out of memory");
                    rc = IO_ERR_OOM;
                    goto done;
                }
                bs.active = 1;
                JxlDecoderSetBoxBuffer(dec, bs.buf, bs.cap);
            }
        } else if (st == JXL_DEC_BOX_NEED_MORE_OUTPUT) {
            size_t remaining = JxlDecoderReleaseBoxBuffer(dec);
            size_t used = bs.cap - remaining;
            size_t ncap = bs.cap * 2;
            uint8_t *nb = (uint8_t *)realloc(bs.buf, ncap);
            if (!nb) {
                io_set_err(err, "out of memory");
                rc = IO_ERR_OOM;
                goto done;
            }
            bs.buf = nb;
            bs.cap = ncap;
            JxlDecoderSetBoxBuffer(dec, bs.buf + used, bs.cap - used);
        }
    }
    if (bs.active) {
        size_t remaining = JxlDecoderReleaseBoxBuffer(dec);
        keep_box(out, &bs, bs.cap - remaining);
    }
    if (!have_image || !out->pixels) {
        io_set_err(err, "JPEG XL file contained no image data");
        rc = IO_ERR_CORRUPT;
        goto done;
    }
    rc = IO_OK;
done:
    free(bs.buf);
    if (runner) JxlThreadParallelRunnerDestroy(runner);
    JxlDecoderDestroy(dec);
    if (rc != IO_OK) io_image_release(out);
    return rc;
}

int shim_jxl_decode_jpeg(const uint8_t *data, size_t len, int threads, io_buffer *jpeg_out,
                         char *err) {
    memset(jpeg_out, 0, sizeof(*jpeg_out));
    int rc = IO_ERR_GENERIC;
    uint8_t *buf = NULL;
    size_t cap = 0;
    int have_recon = 0;
    JxlDecoder *dec = JxlDecoderCreate(NULL);
    void *runner = NULL;
    if (!dec) {
        io_set_err(err, "JxlDecoderCreate failed");
        return IO_ERR_OOM;
    }
    runner = JxlThreadParallelRunnerCreate(NULL, worker_threads(threads));
    if (!runner || JxlDecoderSetParallelRunner(dec, JxlThreadParallelRunner, runner) !=
                       JXL_DEC_SUCCESS) {
        io_set_err(err, "JxlDecoderSetParallelRunner failed");
        goto done;
    }
    if (JxlDecoderSubscribeEvents(dec, JXL_DEC_JPEG_RECONSTRUCTION | JXL_DEC_FULL_IMAGE) !=
        JXL_DEC_SUCCESS) {
        io_set_err(err, "JxlDecoderSubscribeEvents failed");
        goto done;
    }
    if (JxlDecoderSetInput(dec, data, len) != JXL_DEC_SUCCESS) {
        io_set_err(err, "JxlDecoderSetInput failed");
        goto done;
    }
    JxlDecoderCloseInput(dec);

    for (;;) {
        JxlDecoderStatus st = JxlDecoderProcessInput(dec);
        if (st == JXL_DEC_SUCCESS) break;
        if (st == JXL_DEC_ERROR || st == JXL_DEC_NEED_MORE_INPUT) {
            io_set_err(err, "libjxl failed while reconstructing the JPEG");
            rc = IO_ERR_CORRUPT;
            goto done;
        }
        if (st == JXL_DEC_JPEG_RECONSTRUCTION) {
            have_recon = 1;
            cap = len * 2 + 65536;
            buf = (uint8_t *)malloc(cap);
            if (!buf) {
                io_set_err(err, "out of memory");
                rc = IO_ERR_OOM;
                goto done;
            }
            if (JxlDecoderSetJPEGBuffer(dec, buf, cap) != JXL_DEC_SUCCESS) {
                io_set_err(err, "JxlDecoderSetJPEGBuffer failed");
                goto done;
            }
        } else if (st == JXL_DEC_JPEG_NEED_MORE_OUTPUT) {
            size_t remaining = JxlDecoderReleaseJPEGBuffer(dec);
            size_t used = cap - remaining;
            size_t ncap = cap * 2;
            uint8_t *nb = (uint8_t *)realloc(buf, ncap);
            if (!nb) {
                io_set_err(err, "out of memory");
                rc = IO_ERR_OOM;
                goto done;
            }
            buf = nb;
            cap = ncap;
            if (JxlDecoderSetJPEGBuffer(dec, buf + used, cap - used) != JXL_DEC_SUCCESS) {
                io_set_err(err, "JxlDecoderSetJPEGBuffer failed");
                goto done;
            }
        } else if (st == JXL_DEC_NEED_IMAGE_OUT_BUFFER) {
            /* No reconstruction data: this is not a transcoded JPEG. */
            io_set_err(err, "the JPEG XL file carries no JPEG reconstruction data");
            rc = IO_ERR_NO_RECON;
            goto done;
        } else if (st == JXL_DEC_FULL_IMAGE) {
            /* all JPEG bytes have been produced */
        }
    }
    if (!have_recon) {
        io_set_err(err, "the JPEG XL file carries no JPEG reconstruction data");
        rc = IO_ERR_NO_RECON;
        goto done;
    }
    {
        size_t remaining = JxlDecoderReleaseJPEGBuffer(dec);
        jpeg_out->data = buf;
        jpeg_out->len = cap - remaining;
        buf = NULL;
    }
    rc = IO_OK;
done:
    free(buf);
    if (runner) JxlThreadParallelRunnerDestroy(runner);
    JxlDecoderDestroy(dec);
    return rc;
}
